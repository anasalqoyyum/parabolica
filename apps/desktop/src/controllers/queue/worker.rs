use std::collections::{HashMap, HashSet};
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock, mpsc};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use parabolica_audio::engine::{EngineEvent, EngineSnapshot, PlaybackEngine, PlaybackError};
use parabolica_audio::transitions::PlaybackSequence;
use parabolica_domain::control::Generation;
use parabolica_domain::dispatch::coalesce;
use parabolica_domain::ids::{MediaFileId, QueueEntryId};
use parabolica_domain::playback::{PositionStamp, TransportState};
use parabolica_domain::queue::{
    InsertPosition, QueueCommand, QueueEntry, QueueSession, RepeatMode, ShuffleOrder,
};
use parabolica_domain::source::{PlayableSource, ResolvedSource};
use parabolica_storage::playlists::m3u8::IMPORT_BYTE_LIMIT;
use parabolica_storage::playlists::{Playlist, SourceDetails};
use parabolica_storage::queue::{QueueCheckpoint, Shuffle};
use parabolica_storage::session::{
    HISTORY_LIMIT, ListeningRecord, MAX_RETRY_ATTEMPTS, SessionCheckpoint, UnavailableEntry,
};
use parabolica_storage::{Storage, StorageError};

use super::{
    Command, EnqueueMode, HistoryView, PlaylistEntryView, PlaylistView, QueueEntryView,
    QueueSnapshot, SourceView,
};

pub(super) struct Worker {
    storage: Option<Storage>,
    engine: PlaybackEngine,
    engine_events: mpsc::Receiver<EngineEvent>,
    events: mpsc::Sender<EngineEvent>,
    latest: Arc<RwLock<Arc<QueueSnapshot>>>,
    queue: QueueSession,
    position: Duration,
    stamp: Option<PositionStamp>,
    transport: TransportState,
    paused: bool,
    generation: Option<Generation>,
    generation_floor: u64,
    awaiting_engine: bool,
    expected_engine_current: Option<QueueEntryId>,
    pending_start: Option<QueueEntryId>,
    pending_end: bool,
    engine_current: Option<QueueEntryId>,
    engine_transport: TransportState,
    unavailable: HashMap<QueueEntryId, UnavailableEntry>,
    history: Vec<ListeningRecord>,
    catalog: HashMap<PlayableSource, SourceDetails>,
    ephemeral: HashMap<MediaFileId, PathBuf>,
    playlists: Vec<Playlist>,
    likes: Vec<PlayableSource>,
    view: QueueSnapshot,
    checkpoint_at: Instant,
}

impl Worker {
    pub(super) fn new(
        storage: Option<Storage>,
        engine: PlaybackEngine,
        engine_events: mpsc::Receiver<EngineEvent>,
        events: mpsc::Sender<EngineEvent>,
        latest: Arc<RwLock<Arc<QueueSnapshot>>>,
    ) -> Self {
        Self {
            storage,
            engine,
            engine_events,
            events,
            latest,
            queue: QueueSession::default(),
            position: Duration::ZERO,
            stamp: None,
            transport: TransportState::Stopped,
            paused: true,
            generation: None,
            generation_floor: 0,
            awaiting_engine: false,
            expected_engine_current: None,
            pending_start: None,
            pending_end: false,
            engine_current: None,
            engine_transport: TransportState::Stopped,
            unavailable: HashMap::new(),
            history: Vec::new(),
            catalog: HashMap::new(),
            ephemeral: HashMap::new(),
            playlists: Vec::new(),
            likes: Vec::new(),
            view: QueueSnapshot::default(),
            checkpoint_at: Instant::now(),
        }
    }

    pub(super) fn run(mut self, commands: mpsc::Receiver<Command>, shutdown: Arc<AtomicBool>) {
        if let Err(error) = self.restore() {
            self.view.error = Some(error.to_string());
        }
        if let Err(error) = self.refresh_library() {
            self.view.error = Some(error.to_string());
        }
        self.view.ready = true;
        self.publish();
        loop {
            if shutdown.load(Ordering::Acquire) {
                while let Ok(command) = commands.try_recv() {
                    self.capture_position();
                    if let Err(error) = self.command(command) {
                        self.view.error = Some(error.to_string());
                    }
                }
                break;
            }
            self.drain_engine();
            if self.checkpoint_at.elapsed() >= Duration::from_millis(500) {
                self.capture_position();
                self.checkpoint();
                self.publish();
            }
            if self.awaiting_engine {
                match self.engine_events.recv_timeout(Duration::from_millis(5)) {
                    Ok(event) => {
                        self.handle_event(&event);
                        let _ = self.events.send(event);
                    }
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                }
                continue;
            }
            match commands.recv_timeout(Duration::from_millis(5)) {
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
                Ok(command) => {
                    self.capture_position();
                    self.view.error = None;
                    if let Err(error) = self.command(command) {
                        self.view.error = Some(error.to_string());
                    }
                    self.checkpoint();
                    self.publish();
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
            }
        }
        self.capture_position();
        self.paused = true;
        self.engine.pause();
        let deadline = Instant::now() + Duration::from_secs(2);
        while self.engine_current.is_some()
            && matches!(
                self.engine_transport,
                TransportState::Playing | TransportState::Loading
            )
            && Instant::now() < deadline
        {
            match self.engine_events.recv_timeout(Duration::from_millis(10)) {
                Ok(event) => self.handle_event(&event),
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
                Err(mpsc::RecvTimeoutError::Timeout) => {}
            }
        }
        self.capture_position();
        self.checkpoint();
    }

    fn storage(&self) -> Result<&Storage, StorageError> {
        self.storage.as_ref().ok_or(StorageError::Closed)
    }

    fn restore(&mut self) -> Result<(), StorageError> {
        if let Some(storage) = &self.storage
            && let Some(saved) = storage.load_session()?
        {
            self.queue = QueueSession::restore(
                saved.queue.entries,
                saved.queue.current,
                saved.queue.repeat,
                saved.queue.shuffle.map(|shuffle| ShuffleOrder {
                    seed: shuffle.seed,
                    order: shuffle.order,
                }),
                saved.navigation,
            )
            .map_err(|_| StorageError::InvalidStoredValue {
                column: "queue session",
            })?;
            self.position = saved.queue.position;
            self.unavailable = saved
                .unavailable
                .into_iter()
                .map(|entry| (entry.entry, entry))
                .collect();
            self.history = saved.history;
            if self.queue.current().is_some() {
                self.select_current(true)?;
            }
        }
        Ok(())
    }

    fn command(&mut self, command: Command) -> Result<(), StorageError> {
        match command {
            Command::Queue(command) => {
                let previous = self.queue.current();
                let play_now = matches!(command, QueueCommand::PlayNow(_));
                let jump = matches!(command, QueueCommand::Jump(_));
                self.queue
                    .apply(command)
                    .map_err(|error| StorageError::Conflict(error.to_string()))?;
                let ids: HashSet<_> = self.queue.entries().iter().map(|entry| entry.id).collect();
                self.unavailable.retain(|id, _| ids.contains(id));
                if previous != self.queue.current() || play_now || jump {
                    self.position = Duration::ZERO;
                    self.select_current(if play_now || jump { false } else { self.paused })?;
                } else {
                    self.sync_sequence()?;
                }
            }
            Command::Sources(sources, mode) => {
                let at = if mode == EnqueueMode::Later {
                    InsertPosition::End
                } else {
                    InsertPosition::AfterCurrent
                };
                let ids = self.queue.insert(sources, at);
                self.after_insert(&ids, mode)?;
            }
            Command::Files(files, mode) => {
                let mut entries = Vec::new();
                for (id, indexed, path) in files {
                    let file = match (indexed, &self.storage) {
                        (Some(file), _) => file,
                        (None, Some(storage)) => storage.register_file_path(&path)?,
                        (None, None) => self
                            .ephemeral
                            .iter()
                            .find_map(|(id, stored)| (*stored == path).then_some(*id))
                            .unwrap_or_else(MediaFileId::generate),
                    };
                    if self.storage.is_none() {
                        self.ephemeral.insert(file, path);
                    }
                    entries.push(QueueEntry {
                        id,
                        source: PlayableSource::LocalFile(file),
                    });
                }
                let at = if mode == EnqueueMode::Later {
                    InsertPosition::End
                } else {
                    InsertPosition::AfterCurrent
                };
                let ids = self
                    .queue
                    .insert_entries(entries, at)
                    .map_err(|error| StorageError::Conflict(error.to_string()))?;
                self.after_insert(&ids, mode)?;
            }
            Command::Next => self.navigate(false)?,
            Command::Previous => {
                self.queue.previous();
                self.position = Duration::ZERO;
                self.select_current(false)?;
            }
            Command::Retry(entry) => {
                if !self.queue.entries().iter().any(|item| item.id == entry) {
                    return Err(StorageError::NotFound(entry.to_string()));
                }
                if let Some(state) = self.unavailable.get_mut(&entry) {
                    if state.retry_attempts >= MAX_RETRY_ATTEMPTS {
                        return Err(StorageError::Conflict(
                            "Retry limit reached; remove and add this entry to try again".into(),
                        ));
                    }
                    state.retry_attempts += 1;
                    state.reason.clear();
                }
                self.queue
                    .apply(QueueCommand::Jump(entry))
                    .map_err(|error| StorageError::Conflict(error.to_string()))?;
                self.position = Duration::ZERO;
                self.select_current(false)?;
            }
            Command::Pause => {
                self.paused = true;
                self.stamp = None;
                if self.queue.current().is_some() {
                    self.transport = TransportState::Paused;
                }
                self.engine.pause();
            }
            Command::Resume => {
                if let Some(entry) = self.queue.current()
                    && self.engine_current == Some(entry)
                    && !self.is_unavailable(entry)
                {
                    self.paused = false;
                    self.engine.resume();
                } else {
                    if self.queue.current().is_none() {
                        self.queue.next(false);
                    }
                    self.select_current(false)?;
                }
            }
            Command::Seek {
                entry,
                generation,
                position,
            } => {
                if self.queue.current() == Some(entry)
                    && generation == self.generation
                    && self.engine_current == Some(entry)
                {
                    self.position = position;
                    self.stamp = None;
                    self.fence();
                    if let Some(generation) = generation {
                        self.engine.seek_in_generation(generation, entry, position);
                    } else {
                        self.engine.seek(entry, position);
                    }
                }
            }
            Command::Volume(volume) => self.engine.set_volume(volume),
            Command::CreatePlaylist(name) => {
                self.view.last_created_playlist = Some(self.storage()?.create_playlist(&name)?);
                self.refresh_library()?;
            }
            Command::RenamePlaylist(id, name) => {
                self.storage()?.rename_playlist(id, &name)?;
                self.refresh_library()?;
            }
            Command::DeletePlaylist(id) => {
                self.storage()?.delete_playlist(id)?;
                self.refresh_library()?;
            }
            Command::AppendPlaylist(id, sources) => {
                self.storage()?.append_playlist(id, sources)?;
                self.refresh_library()?;
            }
            Command::ReorderPlaylist(id, order) => {
                self.storage()?.reorder_playlist(id, order)?;
                self.refresh_library()?;
            }
            Command::RemovePlaylistEntry(id, entry) => {
                self.storage()?.remove_playlist_entry(id, entry)?;
                self.refresh_library()?;
            }
            Command::Like(source, liked) => {
                self.storage()?.set_local_like(source, liked)?;
                self.refresh_library()?;
            }
            Command::Import(path, name) => {
                let mut bytes = Vec::new();
                File::open(&path)?
                    .take(IMPORT_BYTE_LIMIT as u64 + 1)
                    .read_to_end(&mut bytes)?;
                self.view.last_created_playlist =
                    Some(self.storage()?.import_m3u8(&name, &path, &bytes)?);
                self.refresh_library()?;
            }
            Command::Export(id, destination) => {
                let export = self.storage()?.export_m3u8(id, &destination)?;
                self.view.last_export = Some(export.clone());
                if !export.non_exportable.is_empty() {
                    return Err(StorageError::Conflict(format!(
                        "{} entries cannot be exported to M3U8; no file was written",
                        export.non_exportable.len()
                    )));
                }
                let mut file = OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(destination)?;
                file.write_all(export.text.as_bytes())?;
                file.sync_all()?;
            }
            Command::PlayPlaylist(id, mode) => {
                let playlist = self.storage()?.playlist(id)?;
                self.command(Command::Sources(
                    playlist
                        .entries
                        .into_iter()
                        .map(|entry| entry.source)
                        .collect(),
                    mode,
                ))?;
            }
            Command::Refresh => {
                self.refresh_library()?;
                self.refresh_queue_sources()?;
            }
        }
        Ok(())
    }

    fn after_insert(
        &mut self,
        ids: &[QueueEntryId],
        mode: EnqueueMode,
    ) -> Result<(), StorageError> {
        if mode == EnqueueMode::Now
            && let Some(id) = ids.first()
        {
            self.queue
                .apply(QueueCommand::Jump(*id))
                .map_err(|error| StorageError::Conflict(error.to_string()))?;
            self.position = Duration::ZERO;
            self.select_current(false)?;
        } else {
            self.sync_sequence()?;
        }
        Ok(())
    }

    fn fence(&mut self) {
        self.generation_floor = self.generation.map_or(1, |generation| generation.get() + 1);
        self.awaiting_engine = true;
        self.expected_engine_current = self.queue.current();
        self.pending_end = false;
        self.stamp = None;
    }

    fn stop_engine(&mut self) {
        self.pending_start = None;
        self.fence();
        self.expected_engine_current = None;
        self.engine.stop();
    }

    fn select_current(&mut self, paused: bool) -> Result<(), StorageError> {
        self.paused = paused;
        self.stamp = None;
        let Some(entry) = self.queue.current() else {
            self.position = Duration::ZERO;
            self.transport = TransportState::Stopped;
            self.stop_engine();
            return Ok(());
        };
        self.refresh_queue_sources()?;
        if self.is_unavailable(entry) {
            self.transport = TransportState::Blocked;
            self.view.error = self
                .unavailable
                .get(&entry)
                .map(|entry| entry.reason.clone());
            self.stop_engine();
            return Ok(());
        }
        let sequence = self.sequence(entry)?;
        self.transport = if paused {
            TransportState::Paused
        } else {
            TransportState::Loading
        };
        self.fence();
        self.pending_start = Some(entry);
        self.engine
            .start_sequence(sequence, entry, self.position, paused)
            .map_err(|error| StorageError::Conflict(error.to_string()))?;
        Ok(())
    }

    fn sync_sequence(&mut self) -> Result<(), StorageError> {
        self.refresh_queue_sources()?;
        let Some(entry) = self.queue.current() else {
            return Ok(());
        };
        if self.engine_current != Some(entry) || self.is_unavailable(entry) {
            return self.select_current(self.paused);
        }
        let sequence = self.sequence(entry)?;
        self.fence();
        self.engine.update_sequence(sequence);
        Ok(())
    }

    fn sequence(&self, selected: QueueEntryId) -> Result<PlaybackSequence, StorageError> {
        let mut order = self.queue.effective_order();
        let any_unavailable = order.iter().any(|id| self.is_unavailable(*id));
        let repeat = if any_unavailable {
            let index = order
                .iter()
                .position(|id| *id == selected)
                .ok_or_else(|| StorageError::NotFound(selected.to_string()))?;
            if self.queue.repeat() == RepeatMode::All {
                order.rotate_left(index);
            } else {
                order.drain(..index);
            }
            order = order
                .into_iter()
                .take_while(|id| !self.is_unavailable(*id))
                .collect();
            if self.queue.repeat() == RepeatMode::One {
                RepeatMode::One
            } else {
                RepeatMode::Off
            }
        } else {
            self.queue.repeat()
        };
        let by_id: HashMap<_, _> = self
            .queue
            .entries()
            .iter()
            .map(|entry| (entry.id, entry))
            .collect();
        let entries = order
            .into_iter()
            .map(|id| {
                let entry = by_id
                    .get(&id)
                    .ok_or_else(|| StorageError::NotFound(id.to_string()))?;
                let resolved = self
                    .catalog
                    .get(&entry.source)
                    .and_then(|details| details.resolved.clone())
                    .ok_or(StorageError::Invalid {
                        what: "queue source",
                        reason: "cannot be resolved",
                    })?;
                Ok((id, resolved))
            })
            .collect::<Result<Vec<_>, StorageError>>()?;
        PlaybackSequence::new(entries, repeat)
            .map_err(|error| StorageError::Conflict(error.to_string()))
    }

    fn navigate(&mut self, automatic: bool) -> Result<(), StorageError> {
        self.refresh_queue_sources()?;
        let limit = self.queue.entries().len();
        let mut visited = HashSet::new();
        for _ in 0..limit {
            let Some(id) = self.queue.next(automatic) else {
                break;
            };
            if !visited.insert(id) {
                self.queue.stop();
                break;
            }
            if !self.is_unavailable(id) || automatic {
                break;
            }
        }
        if self
            .queue
            .current()
            .is_some_and(|id| self.is_unavailable(id))
            && !automatic
        {
            self.queue.stop();
        }
        self.position = Duration::ZERO;
        self.select_current(false)
    }

    fn is_unavailable(&self, entry: QueueEntryId) -> bool {
        self.unavailable
            .get(&entry)
            .is_some_and(|state| !state.reason.is_empty())
    }

    fn refresh_queue_sources(&mut self) -> Result<(), StorageError> {
        for entry in self.queue.entries().to_vec() {
            let details = self.lookup_source(&entry.source)?;
            if let Some(reason) = &details.unavailable_reason {
                self.unavailable
                    .entry(entry.id)
                    .or_insert_with(|| UnavailableEntry {
                        entry: entry.id,
                        reason: String::new(),
                        retry_attempts: 0,
                    })
                    .reason = reason.clone();
            }
            self.catalog.insert(entry.source, details);
        }
        Ok(())
    }

    fn lookup_source(&self, source: &PlayableSource) -> Result<SourceDetails, StorageError> {
        let mut details = if let Some(storage) = &self.storage {
            storage.source_details(source)?
        } else {
            let path = match source {
                PlayableSource::LocalFile(id) => self.ephemeral.get(id).cloned(),
                _ => None,
            };
            SourceDetails {
                source: source.clone(),
                title: path
                    .as_ref()
                    .and_then(|path| path.file_stem()?.to_str())
                    .unwrap_or("Unavailable source")
                    .to_owned(),
                source_label: if source.is_local() {
                    "Local file"
                } else {
                    "Provider"
                }
                .into(),
                resolved: match (source, &path) {
                    (PlayableSource::LocalFile(file), Some(path)) => {
                        Some(ResolvedSource::local_file(*file, path.clone()))
                    }
                    _ => None,
                },
                unavailable_reason: path
                    .is_none()
                    .then(|| "Source identity is unavailable".into()),
                path,
            }
        };
        if matches!(source, PlayableSource::CueTrack(_)) {
            details.unavailable_reason = Some("CUE range playback is not available yet".into());
            details.resolved = None;
        } else if details.unavailable_reason.is_none()
            && let Some(path) = &details.path
        {
            match std::fs::metadata(path) {
                Ok(metadata) if metadata.is_file() => {}
                Ok(_) => {
                    details.unavailable_reason = Some("Path is not a file".into());
                }
                Err(error) => {
                    details.unavailable_reason = Some(format!("File is unavailable: {error}"));
                }
            }
        }
        Ok(details)
    }

    fn refresh_library(&mut self) -> Result<(), StorageError> {
        if let Some(storage) = &self.storage {
            self.playlists = storage.playlists()?;
            self.likes = storage.local_likes()?;
        }
        let sources: HashSet<_> = self
            .playlists
            .iter()
            .flat_map(|playlist| playlist.entries.iter().map(|entry| entry.source.clone()))
            .chain(self.likes.iter().cloned())
            .chain(self.history.iter().map(|record| record.source.clone()))
            .collect();
        for source in sources {
            self.catalog
                .insert(source.clone(), self.lookup_source(&source)?);
        }
        Ok(())
    }

    fn capture_position(&mut self) {
        if let Some(stamp) = self.stamp {
            self.position = stamp.position_at(Instant::now());
        }
    }

    fn checkpoint(&mut self) {
        if let Some(storage) = &self.storage {
            let session = SessionCheckpoint {
                queue: QueueCheckpoint {
                    entries: self.queue.entries().to_vec(),
                    current: self.queue.current(),
                    position: self.position,
                    repeat: self.queue.repeat(),
                    shuffle: self.queue.shuffle().map(|shuffle| Shuffle {
                        seed: shuffle.seed,
                        order: shuffle.order.clone(),
                    }),
                },
                navigation: self.queue.history().to_vec(),
                unavailable: self.unavailable.values().cloned().collect(),
                history: self.history.clone(),
            };
            if let Err(error) = storage.save_session(session) {
                self.view.error = Some(error.to_string());
            }
        }
        self.checkpoint_at = Instant::now();
    }

    fn drain_engine(&mut self) {
        let mut batch = Vec::new();
        while let Ok(event) = self.engine_events.try_recv() {
            self.handle_event(&event);
            batch.push(event);
        }
        for event in coalesce(batch) {
            let _ = self.events.send(event);
        }
    }

    fn handle_event(&mut self, event: &EngineEvent) {
        match event {
            EngineEvent::Snapshot(snapshot) => self.engine_snapshot(&snapshot.state),
            EngineEvent::Position(stamp)
                if !self.awaiting_engine
                    && self.engine_current == self.queue.current()
                    && self.queue.current().is_some() =>
            {
                self.stamp = Some(*stamp);
                self.capture_position();
            }
            EngineEvent::EntryBoundary {
                generation,
                from,
                to,
                ..
            } if generation.get() >= self.generation_floor => {
                if let Some(to) = to {
                    if self.queue.entries().iter().any(|entry| entry.id == *to) {
                        if from.is_some() {
                            self.started(*to);
                            self.position = Duration::ZERO;
                            self.stamp = None;
                        } else if self.queue.current() == Some(*to) {
                            self.pending_start = Some(*to);
                        }
                    }
                } else {
                    self.pending_end = true;
                }
                self.checkpoint();
            }
            _ => {}
        }
        self.publish();
    }

    fn engine_snapshot(&mut self, snapshot: &EngineSnapshot) {
        let generation = snapshot.playback.generation;
        let output_open_failure = snapshot.playback.current.is_none()
            && snapshot.failure.as_ref().is_some_and(|failure| {
                matches!(failure.error, PlaybackError::Output(_))
                    && failure.entry == self.queue.current()
            });
        if self.awaiting_engine
            && !output_open_failure
            && (snapshot.failure.is_some()
                || snapshot.playback.current != self.expected_engine_current)
        {
            return;
        }
        if generation.get() < self.generation_floor && !output_open_failure {
            return;
        }
        if self.generation.is_some_and(|current| generation < current) {
            return;
        }
        self.generation = Some(generation);
        self.generation_floor = generation.get();
        self.awaiting_engine = false;
        self.engine_current = snapshot.playback.current;
        self.engine_transport = snapshot.playback.transport;
        if let Some(failure) = &snapshot.failure {
            if snapshot.playback.current == self.queue.current()
                && let Some(stamp) = snapshot.playback.position
            {
                self.position = stamp.position_at(stamp.captured_at);
            }
            self.view.error = Some(failure.error.to_string());
            if !matches!(failure.error, PlaybackError::Output(_))
                && let Some(entry) = failure.entry
                && self
                    .queue
                    .entries()
                    .iter()
                    .any(|candidate| candidate.id == entry)
            {
                self.unavailable
                    .entry(entry)
                    .or_insert_with(|| UnavailableEntry {
                        entry,
                        reason: String::new(),
                        retry_attempts: 0,
                    })
                    .reason = failure.error.to_string();
                let _ = self.queue.apply(QueueCommand::Jump(entry));
                self.position = Duration::ZERO;
            }
            self.pending_end = false;
            self.pending_start = None;
            self.stamp = None;
            self.transport = TransportState::Blocked;
            self.checkpoint();
            return;
        }
        if self.pending_end {
            self.pending_end = false;
            if let Err(error) = self.navigate(true) {
                self.view.error = Some(error.to_string());
            }
            self.checkpoint();
            return;
        }
        if snapshot.playback.current != self.queue.current() {
            return;
        }
        if self.queue.current().is_some() {
            self.transport = snapshot.playback.transport;
            self.paused = !matches!(
                self.transport,
                TransportState::Playing | TransportState::Loading
            );
            if let Some(stamp) = snapshot.playback.position {
                self.stamp = Some(stamp);
                self.capture_position();
            }
        }
        if snapshot.playback.transport == TransportState::Playing
            && let Some(entry) = self.pending_start.take()
        {
            self.started(entry);
        }
        if matches!(
            snapshot.playback.transport,
            TransportState::Paused | TransportState::Stopped
        ) {
            self.checkpoint();
        }
    }

    fn started(&mut self, id: QueueEntryId) {
        self.pending_start = None;
        if self.queue.record_started(id).is_err() {
            return;
        }
        if let Some(entry) = self.queue.entries().iter().find(|entry| entry.id == id) {
            self.history.push(ListeningRecord {
                entry: id,
                source: entry.source.clone(),
                started_at_ms: unix_ms(),
            });
            if self.history.len() > HISTORY_LIMIT {
                self.history.remove(0);
            }
            if let Some(state) = self.unavailable.get_mut(&id) {
                state.reason.clear();
            }
        }
        self.checkpoint();
    }

    fn source_view(&self, source: &PlayableSource) -> SourceView {
        self.catalog.get(source).map_or_else(
            || SourceView {
                source: source.clone(),
                title: "Unavailable source".into(),
                source_label: if source.is_local() {
                    "Local file"
                } else {
                    "Provider"
                }
                .into(),
                path: None,
                unavailable_reason: Some("Source has not been resolved".into()),
            },
            |details| SourceView {
                source: source.clone(),
                title: details.title.clone(),
                source_label: details.source_label.clone(),
                path: details.path.clone(),
                unavailable_reason: details.unavailable_reason.clone(),
            },
        )
    }

    fn publish(&mut self) {
        let by_id: HashMap<_, _> = self
            .queue
            .entries()
            .iter()
            .map(|entry| (entry.id, entry))
            .collect();
        self.view.entries = self
            .queue
            .effective_order()
            .iter()
            .filter_map(|id| by_id.get(id).copied())
            .map(|entry| {
                let mut details = self.source_view(&entry.source);
                let state = self.unavailable.get(&entry.id);
                if let Some(state) = state
                    && !state.reason.is_empty()
                {
                    details.unavailable_reason = Some(state.reason.clone());
                }
                QueueEntryView {
                    entry: entry.clone(),
                    details,
                    retry_attempts: state.map_or(0, |state| state.retry_attempts),
                }
            })
            .collect();
        self.view.current = self.queue.current();
        self.view.position = self.position;
        self.view.transport = self.transport;
        self.view.generation = self.generation;
        self.view.repeat = self.queue.repeat();
        self.view.shuffle_seed = self.queue.shuffle().map(|shuffle| shuffle.seed);
        self.view.playlists = self
            .playlists
            .iter()
            .map(|playlist| PlaylistView {
                id: playlist.id,
                name: playlist.name.clone(),
                entries: playlist
                    .entries
                    .iter()
                    .map(|entry| PlaylistEntryView {
                        id: entry.id,
                        details: self.source_view(&entry.source),
                    })
                    .collect(),
            })
            .collect();
        self.view.likes = self
            .likes
            .iter()
            .map(|source| self.source_view(source))
            .collect();
        self.view.history = self
            .history
            .iter()
            .map(|record| HistoryView {
                record: record.clone(),
                details: self.source_view(&record.source),
            })
            .collect();
        self.view.storage_available = self.storage.is_some();
        let mut latest = self.latest.write().expect("queue snapshot lock");
        self.view.revision = latest.revision + 1;
        *latest = Arc::new(self.view.clone());
    }
}

fn unix_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(i64::MAX as u128) as i64)
        .unwrap_or_default()
}

#[cfg(test)]
mod event_tests {
    use super::*;
    use parabolica_audio::engine::{NoOutput, OutputError, PlaybackFailure};
    use parabolica_domain::control::{GenerationCounter, SnapshotSequencer};
    use parabolica_domain::format::SampleRate;
    use parabolica_domain::gain::Volume;
    use parabolica_domain::playback::PlaybackSnapshot;

    fn worker() -> Worker {
        let (engine, engine_events) = PlaybackEngine::start(Box::new(NoOutput));
        let (events, _receiver) = mpsc::channel();
        Worker::new(
            None,
            engine,
            engine_events,
            events,
            Arc::new(RwLock::new(Arc::new(QueueSnapshot::default()))),
        )
    }

    fn snapshot(generation: Generation, current: QueueEntryId, frame: u64) -> EngineSnapshot {
        EngineSnapshot {
            playback: PlaybackSnapshot {
                generation,
                transport: TransportState::Playing,
                current: Some(current),
                position: Some(PositionStamp {
                    frame,
                    rate: SampleRate::new(48_000).unwrap(),
                    captured_at: Instant::now(),
                    advancing: false,
                }),
                duration: None,
                route: None,
                signal_path: None,
                gain: None,
                volume: Volume::UNITY,
            },
            path: None,
            source: None,
            output: None,
            underruns: 0,
            failure: None,
        }
    }

    #[test]
    fn old_generations_and_seeks_cannot_change_a_new_selection() {
        let mut worker = worker();
        let ids = worker.queue.insert(
            [
                PlayableSource::LocalFile(MediaFileId::generate()),
                PlayableSource::LocalFile(MediaFileId::generate()),
            ],
            InsertPosition::End,
        );
        let counter = GenerationCounter::default();
        let old = counter.advance();
        let new = counter.advance();
        worker.queue.apply(QueueCommand::Jump(ids[1])).unwrap();
        worker.generation = Some(new);
        worker.generation_floor = new.get();
        worker.engine_current = Some(ids[1]);
        worker.position = Duration::from_secs(2);
        let mut sequencer = SnapshotSequencer::default();
        worker.handle_event(&EngineEvent::Snapshot(Box::new(
            sequencer.wrap(snapshot(old, ids[0], 99_000)),
        )));
        worker.handle_event(&EngineEvent::EntryBoundary {
            generation: old,
            from: Some(ids[1]),
            to: Some(ids[0]),
            output_frame: 5,
        });
        worker
            .command(Command::Seek {
                entry: ids[1],
                generation: Some(old),
                position: Duration::from_secs(9),
            })
            .unwrap();
        assert_eq!(worker.queue.current(), Some(ids[1]));
        assert_eq!(worker.position, Duration::from_secs(2));
        assert!(worker.history.is_empty());
    }

    #[test]
    fn a_failure_in_flight_cannot_acknowledge_a_new_selection() {
        let mut worker = worker();
        let ids = worker.queue.insert(
            [
                PlayableSource::LocalFile(MediaFileId::generate()),
                PlayableSource::LocalFile(MediaFileId::generate()),
            ],
            InsertPosition::End,
        );
        let counter = GenerationCounter::default();
        let old = counter.advance();
        worker.generation = Some(old);
        worker.queue.apply(QueueCommand::Jump(ids[1])).unwrap();
        worker.fence();
        let failure_generation = counter.advance();
        let mut failed = snapshot(failure_generation, ids[0], 0);
        failed.failure = Some(PlaybackFailure {
            entry: Some(ids[0]),
            path: None,
            error: PlaybackError::Decode("Old source failed".into()),
        });
        worker.engine_snapshot(&failed);
        assert_eq!(worker.queue.current(), Some(ids[1]));
        assert!(worker.awaiting_engine);
        assert!(worker.unavailable.is_empty());
        let new = counter.advance();
        worker.engine_snapshot(&snapshot(new, ids[1], 0));
        assert!(!worker.awaiting_engine);
        assert_eq!(worker.generation, Some(new));
    }

    #[test]
    fn upcoming_failures_name_the_failed_entry_and_output_failures_keep_sources_healthy() {
        let mut worker = worker();
        let ids = worker.queue.insert(
            [
                PlayableSource::LocalFile(MediaFileId::generate()),
                PlayableSource::LocalFile(MediaFileId::generate()),
            ],
            InsertPosition::End,
        );
        worker.queue.record_started(ids[0]).unwrap();
        let generation = GenerationCounter::default().advance();
        let mut state = snapshot(generation, ids[0], 48_000);
        state.failure = Some(PlaybackFailure {
            entry: Some(ids[1]),
            path: None,
            error: PlaybackError::Decode("Corrupt file".into()),
        });
        worker.engine_snapshot(&state);
        assert_eq!(worker.queue.current(), Some(ids[1]));
        assert!(worker.is_unavailable(ids[1]));
        assert!(!worker.is_unavailable(ids[0]));
        assert!(worker.history.is_empty());
        worker.unavailable.clear();
        state.failure = Some(PlaybackFailure {
            entry: Some(ids[0]),
            path: None,
            error: PlaybackError::Output(OutputError::DeviceLost),
        });
        worker.engine_snapshot(&state);
        assert!(worker.unavailable.is_empty());
    }
}
