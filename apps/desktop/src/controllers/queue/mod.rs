//! Nonblocking queue commands. The worker owns storage, source resolution and engine events.

#[cfg(test)]
mod tests;
mod worker;

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock, mpsc};
use std::thread::JoinHandle;
use std::time::Duration;

use parabolica_audio::engine::{EngineEvent, Output, PlaybackEngine};
use parabolica_domain::control::Generation;
use parabolica_domain::gain::Volume;
use parabolica_domain::ids::{MediaFileId, PlaylistEntryId, PlaylistId, QueueEntryId};
use parabolica_domain::playback::TransportState;
use parabolica_domain::queue::{QueueCommand, QueueEntry, RepeatMode};
use parabolica_domain::source::PlayableSource;
use parabolica_storage::Storage;
use parabolica_storage::playlists::m3u8::M3u8Export;
use parabolica_storage::session::ListeningRecord;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnqueueMode {
    Now,
    Next,
    Later,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceView {
    pub source: PlayableSource,
    pub title: String,
    pub source_label: String,
    pub path: Option<PathBuf>,
    pub unavailable_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueueEntryView {
    pub entry: QueueEntry,
    pub details: SourceView,
    pub retry_attempts: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaylistEntryView {
    pub id: PlaylistEntryId,
    pub details: SourceView,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaylistView {
    pub id: PlaylistId,
    pub name: String,
    pub entries: Vec<PlaylistEntryView>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryView {
    pub record: ListeningRecord,
    pub details: SourceView,
}

#[derive(Debug, Clone)]
pub struct QueueSnapshot {
    pub revision: u64,
    /// Entries in effective playback order, so move indices match the displayed rows.
    pub entries: Vec<QueueEntryView>,
    pub current: Option<QueueEntryId>,
    pub position: Duration,
    pub transport: TransportState,
    pub generation: Option<Generation>,
    pub repeat: RepeatMode,
    pub shuffle_seed: Option<u64>,
    pub playlists: Vec<PlaylistView>,
    pub likes: Vec<SourceView>,
    pub history: Vec<HistoryView>,
    pub error: Option<String>,
    pub storage_available: bool,
    pub ready: bool,
    pub last_created_playlist: Option<PlaylistId>,
    pub last_export: Option<M3u8Export>,
}

impl Default for QueueSnapshot {
    fn default() -> Self {
        Self {
            revision: 0,
            entries: Vec::new(),
            current: None,
            position: Duration::ZERO,
            transport: TransportState::Stopped,
            generation: None,
            repeat: RepeatMode::Off,
            shuffle_seed: None,
            playlists: Vec::new(),
            likes: Vec::new(),
            history: Vec::new(),
            error: None,
            storage_available: false,
            ready: false,
            last_created_playlist: None,
            last_export: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct QueueService {
    inner: Arc<Handle>,
}

#[derive(Debug)]
struct Handle {
    inbox: mpsc::SyncSender<Command>,
    latest: Arc<RwLock<Arc<QueueSnapshot>>>,
    thread: Mutex<Option<JoinHandle<()>>>,
    shutdown: Arc<AtomicBool>,
}

#[derive(Debug)]
enum Command {
    Queue(QueueCommand),
    Sources(Vec<PlayableSource>, EnqueueMode),
    Files(
        Vec<(QueueEntryId, Option<MediaFileId>, PathBuf)>,
        EnqueueMode,
    ),
    Next,
    Previous,
    Retry(QueueEntryId),
    Pause,
    Resume,
    Seek {
        entry: QueueEntryId,
        generation: Option<Generation>,
        position: Duration,
    },
    Volume(Volume),
    CreatePlaylist(String),
    RenamePlaylist(PlaylistId, String),
    DeletePlaylist(PlaylistId),
    AppendPlaylist(PlaylistId, Vec<PlayableSource>),
    ReorderPlaylist(PlaylistId, Vec<PlaylistEntryId>),
    RemovePlaylistEntry(PlaylistId, PlaylistEntryId),
    Import(PathBuf, String),
    Export(PlaylistId, PathBuf),
    PlayPlaylist(PlaylistId, EnqueueMode),
    Like(PlayableSource, bool),
    Refresh,
}

impl QueueService {
    pub fn start(
        storage: Option<Storage>,
        output: Box<dyn Output>,
    ) -> (Self, mpsc::Receiver<EngineEvent>) {
        let (engine, engine_events) = PlaybackEngine::start(output);
        let (events, receiver) = mpsc::channel();
        let (inbox, commands) = mpsc::sync_channel(64);
        let latest = Arc::new(RwLock::new(Arc::new(QueueSnapshot::default())));
        let state = latest.clone();
        let shutdown = Arc::new(AtomicBool::new(false));
        let worker_shutdown = shutdown.clone();
        let thread = std::thread::Builder::new()
            .name("parabolica-queue".into())
            .spawn(move || {
                worker::Worker::new(storage, engine, engine_events, events, state)
                    .run(commands, worker_shutdown);
            })
            .expect("the queue worker starts");
        (
            Self {
                inner: Arc::new(Handle {
                    inbox,
                    latest,
                    thread: Mutex::new(Some(thread)),
                    shutdown,
                }),
            },
            receiver,
        )
    }

    pub fn shutdown(&self) {
        let mut thread = self.inner.thread.lock().expect("queue worker handle");
        if let Some(thread) = thread.take() {
            self.inner.shutdown.store(true, Ordering::Release);
            let _ = thread.join();
        }
    }

    /// An in-memory Arc clone. Reading a snapshot never opens files or calls storage.
    pub fn snapshot(&self) -> Arc<QueueSnapshot> {
        self.inner
            .latest
            .read()
            .expect("queue snapshot lock")
            .clone()
    }

    fn send(&self, command: Command) -> bool {
        if self.inner.inbox.try_send(command).is_ok() {
            return true;
        }
        let mut latest = self.inner.latest.write().expect("queue snapshot lock");
        let mut snapshot = (**latest).clone();
        snapshot.revision += 1;
        snapshot.error = Some("Queue worker is busy or closed; try again".into());
        *latest = Arc::new(snapshot);
        false
    }

    pub fn queue(&self, command: QueueCommand) -> bool {
        self.send(Command::Queue(command))
    }
    pub fn enqueue_sources(&self, sources: Vec<PlayableSource>, mode: EnqueueMode) -> bool {
        self.send(Command::Sources(sources, mode))
    }
    pub fn enqueue_files(&self, paths: Vec<PathBuf>, mode: EnqueueMode) -> Vec<QueueEntryId> {
        let files: Vec<_> = paths
            .into_iter()
            .map(|path| (QueueEntryId::generate(), None, path))
            .collect();
        let ids = files.iter().map(|(entry, _, _)| *entry).collect();
        self.send(Command::Files(files, mode));
        ids
    }
    pub fn jump(&self, entry: QueueEntryId) -> bool {
        self.queue(QueueCommand::Jump(entry))
    }
    pub fn remove(&self, entry: QueueEntryId) -> bool {
        self.queue(QueueCommand::Remove(entry))
    }
    pub fn move_entry(&self, entry: QueueEntryId, to_index: usize) -> bool {
        self.queue(QueueCommand::Move { entry, to_index })
    }
    pub fn clear(&self) -> bool {
        self.queue(QueueCommand::Clear)
    }
    pub fn set_shuffle(&self, seed: Option<u64>) -> bool {
        self.queue(QueueCommand::SetShuffle { seed })
    }
    pub fn set_repeat(&self, repeat: RepeatMode) -> bool {
        self.queue(QueueCommand::SetRepeat(repeat))
    }
    pub fn next(&self) -> bool {
        self.send(Command::Next)
    }
    pub fn previous(&self) -> bool {
        self.send(Command::Previous)
    }
    pub fn retry(&self, entry: QueueEntryId) -> bool {
        self.send(Command::Retry(entry))
    }
    pub fn skip(&self) -> bool {
        self.next()
    }
    pub fn pause(&self) -> bool {
        self.send(Command::Pause)
    }
    pub fn resume(&self) -> bool {
        self.send(Command::Resume)
    }
    pub fn seek(&self, entry: QueueEntryId, position: Duration) -> bool {
        self.send(Command::Seek {
            entry,
            generation: self.snapshot().generation,
            position,
        })
    }
    pub fn set_volume(&self, volume: Volume) -> bool {
        self.send(Command::Volume(volume))
    }
    pub fn create_playlist(&self, name: String) -> bool {
        self.send(Command::CreatePlaylist(name))
    }
    pub fn rename_playlist(&self, id: PlaylistId, name: String) -> bool {
        self.send(Command::RenamePlaylist(id, name))
    }
    pub fn delete_playlist(&self, id: PlaylistId) -> bool {
        self.send(Command::DeletePlaylist(id))
    }
    pub fn append_playlist(&self, id: PlaylistId, sources: Vec<PlayableSource>) -> bool {
        self.send(Command::AppendPlaylist(id, sources))
    }
    pub fn reorder_playlist(&self, id: PlaylistId, order: Vec<PlaylistEntryId>) -> bool {
        self.send(Command::ReorderPlaylist(id, order))
    }
    pub fn remove_playlist_entry(&self, id: PlaylistId, entry: PlaylistEntryId) -> bool {
        self.send(Command::RemovePlaylistEntry(id, entry))
    }
    pub fn import_m3u8(&self, path: PathBuf, name: String) -> bool {
        self.send(Command::Import(path, name))
    }
    pub fn export_m3u8(&self, id: PlaylistId, destination: PathBuf) -> bool {
        self.send(Command::Export(id, destination))
    }
    pub fn play_playlist(&self, id: PlaylistId, mode: EnqueueMode) -> bool {
        self.send(Command::PlayPlaylist(id, mode))
    }
    pub fn set_local_like(&self, source: PlayableSource, liked: bool) -> bool {
        self.send(Command::Like(source, liked))
    }
    pub fn refresh(&self) -> bool {
        self.send(Command::Refresh)
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Release);
        if let Some(thread) = self.thread.get_mut().expect("queue worker handle").take() {
            let _ = thread.join();
        }
    }
}
