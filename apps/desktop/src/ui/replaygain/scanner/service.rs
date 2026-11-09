use parabolica_audio::decode::Decoder;
use parabolica_domain::{
    control::CancelToken,
    gain::PeakMethod,
    ids::{JobId, MediaFileId},
};
use parabolica_replaygain::analyze::{self, Analyzer};
use parabolica_storage::{
    Storage,
    replaygain::{ReviewedGroup, ScanCandidate, ScanMode, ScanRecord, ScanState},
};
use std::collections::HashSet;
use std::io;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
    mpsc::{self, SyncSender},
};
use std::thread::{self, JoinHandle};
use std::time::Duration;

#[derive(Debug, Clone, Default)]
pub struct Status {
    pub revision: u64,
    pub completed_scan: Option<JobId>,
    pub busy: bool,
    pub cancellable: bool,
    pub operation: String,
    pub current: String,
    pub frames: u64,
    pub total_frames: Option<u64>,
    pub done: usize,
    pub groups_done: usize,
    pub groups_total: usize,
    pub total: usize,
    pub reused: usize,
    pub waiting_for_playback: bool,
    pub candidates: Arc<Vec<ScanCandidate>>,
    pub preview: Arc<Vec<ReviewedGroup>>,
    pub history: Arc<Vec<ScanRecord>>,
    pub error: Option<String>,
}
pub enum Command {
    Refresh,
    Review(HashSet<MediaFileId>, ScanMode),
    Start(Vec<ReviewedGroup>, ScanMode, PeakMethod),
    Save(JobId),
    Shutdown,
}
pub struct ScannerService {
    sender: SyncSender<Command>,
    status: Arc<Mutex<Status>>,
    cancel: Arc<Mutex<CancelToken>>,
    playing: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}
impl ScannerService {
    pub fn start(storage: Storage) -> Self {
        let (sender, receiver) = mpsc::sync_channel(8);
        let status = Arc::new(Mutex::new(Status::default()));
        let shared = status.clone();
        let cancel = Arc::new(Mutex::new(CancelToken::new()));
        let worker_cancel = cancel.clone();
        let playing = Arc::new(AtomicBool::new(false));
        let worker_playing = playing.clone();
        let thread = thread::spawn(move || {
            // Interrupted runs remain visible and can be resumed as a newly reviewed job.
            match storage.scan_history() {
                Ok(history) => {
                    for run in history {
                        if matches!(run.state, ScanState::Queued | ScanState::Scanning)
                            && let Err(error) = storage.set_scan_state(
                                run.id,
                                ScanState::Cancelled,
                                Some("Interrupted before completion; review and resume".into()),
                            )
                        {
                            update(&shared, |s| s.error = Some(error.to_string()));
                        }
                    }
                }
                Err(error) => update(&shared, |s| s.error = Some(error.to_string())),
            }
            while let Ok(command) = receiver.recv() {
                if matches!(command, Command::Shutdown) {
                    break;
                }
                let token = worker_cancel
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .clone();
                let result = match command {
                    Command::Refresh => reload(&storage, &shared),
                    Command::Review(selected, mode) => (|| {
                        let all = storage.scan_candidates()?;
                        let preview = super::grouping::review(&storage, &all, &selected, mode)?;
                        update(&shared, |s| {
                            s.candidates = Arc::new(all);
                            s.preview = Arc::new(preview);
                            s.completed_scan = None;
                        });
                        Ok(())
                    })()
                    .map_err(|e: parabolica_storage::StorageError| e.to_string()),
                    Command::Start(groups, mode, method) => run(
                        &storage,
                        &shared,
                        &token,
                        &worker_playing,
                        groups,
                        mode,
                        method,
                    ),
                    Command::Save(id) => save(&storage, &shared, &token, &worker_playing, id),
                    Command::Shutdown => unreachable!(),
                };
                if let Err(error) = result {
                    update(&shared, |s| s.error = Some(error));
                }
                update(&shared, |s| s.cancellable = false);
                let _ =
                    reload(&storage, &shared).map_err(|e| update(&shared, |s| s.error = Some(e)));
                update(&shared, |s| {
                    s.busy = false;
                    s.current.clear();
                    s.waiting_for_playback = false;
                });
            }
        });
        let this = Self {
            sender,
            status,
            cancel,
            playing,
            thread: Some(thread),
        };
        let _ = this.send(Command::Refresh);
        this
    }
    pub fn send(&self, command: Command) -> Result<(), String> {
        let mut status = self.status.lock().unwrap_or_else(|e| e.into_inner());
        if status.busy {
            return Err("A scanner operation is already running".into());
        }
        *self.cancel.lock().unwrap_or_else(|e| e.into_inner()) = CancelToken::new();
        status.busy = true;
        status.cancellable = matches!(&command, Command::Start(..) | Command::Save(_));
        if matches!(&command, Command::Refresh | Command::Review(..)) {
            status.completed_scan = None;
        }
        status.operation = match &command {
            Command::Refresh => "Refreshing library",
            Command::Review(..) => "Reviewing groups",
            Command::Start(..) => "Queued",
            Command::Save(_) => "Validating files",
            Command::Shutdown => "Closing",
        }
        .into();
        status.error = None;
        status.revision += 1;
        if self.sender.try_send(command).is_err() {
            status.busy = false;
            status.cancellable = false;
            return Err("Scanner command queue is full".into());
        }
        Ok(())
    }
    pub fn clear_review(&self) {
        update(&self.status, |s| {
            s.preview = Arc::default();
            s.completed_scan = None;
        });
    }
    pub fn cancel(&self) {
        self.cancel
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .cancel();
    }
    pub fn set_playing(&self, playing: bool) {
        self.playing.store(playing, Ordering::Release);
    }
    pub fn status(&self) -> Status {
        self.status
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }
}
impl Drop for ScannerService {
    fn drop(&mut self) {
        self.cancel();
        let _ = self.sender.send(Command::Shutdown);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
fn update(status: &Mutex<Status>, f: impl FnOnce(&mut Status)) {
    let mut status = status.lock().unwrap_or_else(|e| e.into_inner());
    f(&mut status);
    status.revision += 1;
}
fn reload(storage: &Storage, status: &Mutex<Status>) -> Result<(), String> {
    let candidates = storage.scan_candidates().map_err(|e| e.to_string())?;
    let history = storage.scan_history().map_err(|e| e.to_string())?;
    update(status, |s| {
        s.candidates = Arc::new(candidates);
        s.history = Arc::new(history);
    });
    Ok(())
}
fn checkpoint(
    cancel: &CancelToken,
    playing: &AtomicBool,
    status: &Mutex<Status>,
) -> io::Result<()> {
    loop {
        if cancel.is_cancelled() {
            return Err(io::Error::new(io::ErrorKind::Interrupted, "Scan cancelled"));
        }
        let waiting = playing.load(Ordering::Acquire);
        {
            let mut s = status.lock().unwrap_or_else(|e| e.into_inner());
            if s.waiting_for_playback != waiting {
                s.waiting_for_playback = waiting;
                s.revision += 1;
            }
        }
        if !waiting {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(50));
    }
}
fn run(
    storage: &Storage,
    status: &Mutex<Status>,
    cancel: &CancelToken,
    playing: &AtomicBool,
    groups: Vec<ReviewedGroup>,
    mode: ScanMode,
    method: PeakMethod,
) -> Result<(), String> {
    let scan = storage
        .create_scan(groups.clone(), mode, method)
        .map_err(|e| e.to_string())?;
    storage
        .set_scan_state(scan, ScanState::Scanning, None)
        .map_err(|e| e.to_string())?;
    update(status, |s| {
        s.operation = "Scanning".into();
        s.done = 0;
        s.groups_done = 0;
        s.groups_total = if mode == ScanMode::Tracks {
            0
        } else {
            groups.len()
        };
        s.frames = 0;
        s.total_frames = None;
        s.current.clear();
        s.total = groups.iter().map(|g| g.members.len()).sum();
        s.reused = 0;
    });
    update(status, |s| s.completed_scan = Some(scan));
    let result = run_members(
        storage,
        status,
        cancel,
        playing,
        scan,
        &groups,
        (mode, method),
    );
    if let Err(error) = result {
        let state = if cancel.is_cancelled() {
            ScanState::Cancelled
        } else {
            ScanState::Failed
        };
        storage
            .set_scan_state(scan, state, Some(error.clone()))
            .map_err(|e| e.to_string())?;
        if state == ScanState::Failed {
            return Err(error);
        }
    }
    update(status, |s| s.completed_scan = Some(scan));
    Ok(())
}
fn run_members(
    storage: &Storage,
    status: &Mutex<Status>,
    cancel: &CancelToken,
    playing: &AtomicBool,
    scan: JobId,
    groups: &[ReviewedGroup],
    configuration: (ScanMode, PeakMethod),
) -> Result<(), String> {
    let (mode, method) = configuration;
    for (ordinal, group) in groups.iter().enumerate() {
        let mut tracks = Vec::new();
        for member in &group.members {
            update(status, |s| {
                s.current = member.title.clone();
                s.frames = 0;
                s.total_frames = None;
            });
            checkpoint(cancel, playing, status).map_err(|e| e.to_string())?;
            let result = (|| {
                let path = storage
                    .resolve_local_file(member.file)
                    .map_err(source_error)?;
                update(status, |s| s.current = path.display().to_string());
                let before = analyze::fingerprint(&path, || checkpoint(cancel, playing, status))
                    .map_err(|e| e.to_string())?;
                let cached = storage
                    .cached_scan_measurement(member.file, before.clone(), method)
                    .map_err(|e| e.to_string())?;
                let reused = cached.is_some();
                let cached = match cached {
                    Some(cached) => cached,
                    None => {
                        let mut decoder = Decoder::open(&path).map_err(|e| e.to_string())?;
                        let info = decoder.info();
                        let mask = info
                            .positioned_channel_mask()
                            .ok_or("Unpositioned channels cannot be measured")?;
                        let mut analyzer = Analyzer::new(info.format.rate.hz(), mask, method)
                            .map_err(|e| e.to_string())?;
                        update(status, |s| s.total_frames = info.frames);
                        let mut pcm = Vec::new();
                        loop {
                            checkpoint(cancel, playing, status).map_err(|e| e.to_string())?;
                            pcm.clear();
                            if decoder.next_block(&mut pcm).map_err(|e| e.to_string())? == 0 {
                                break;
                            }
                            // Decoder packets can be large. Cancellation is checked at 4096-frame blocks.
                            let channels = decoder.info().format.channels.get() as usize;
                            for block in pcm.chunks(4096 * channels) {
                                checkpoint(cancel, playing, status).map_err(|e| e.to_string())?;
                                analyzer.add(block).map_err(|e| e.to_string())?;
                                update(status, |s| s.frames += (block.len() / channels) as u64);
                            }
                        }
                        if decoder.skipped_packet_count() > 0 {
                            return Err("Damaged audio packets were skipped; complete analysis is unavailable".into());
                        }
                        let analysis = analyzer.finish().map_err(|e| e.to_string())?;
                        let after =
                            analyze::fingerprint(&path, || checkpoint(cancel, playing, status))
                                .map_err(|e| e.to_string())?;
                        if after != before {
                            return Err("File changed during analysis; rescan it".to_string());
                        }
                        storage
                            .cache_scan_measurement(member.file, before, analysis)
                            .map_err(|e| e.to_string())?
                    }
                };
                storage
                    .record_scan_member(scan, member.file, Some(cached.id), None, reused)
                    .map_err(|e| e.to_string())?;
                if reused {
                    update(status, |s| s.reused += 1);
                }
                Ok::<_, String>(cached.analysis)
            })();
            if cancel.is_cancelled() {
                return Err("Scan cancelled".into());
            }
            match result {
                Ok(track) => tracks.push(track),
                Err(error) => {
                    storage
                        .record_scan_member(scan, member.file, None, Some(error), false)
                        .map_err(|e| e.to_string())?;
                }
            }
            update(status, |s| s.done += 1);
        }
        if mode != ScanMode::Tracks && tracks.len() == group.members.len() {
            storage
                .record_scan_album(scan, ordinal, analyze::album(&tracks))
                .map_err(|e| e.to_string())?;
        }
        if mode != ScanMode::Tracks {
            update(status, |s| s.groups_done += 1);
        }
    }
    storage
        .finish_scan(scan)
        .map(|_| ())
        .map_err(|e| e.to_string())
}
fn source_error(error: parabolica_storage::StorageError) -> String {
    match error {
        parabolica_storage::StorageError::NotFound(_) => "File is missing or its folder is offline. Restore it or locate it in the library, then rescan.".into(),
        error => error.to_string(),
    }
}

fn save(
    storage: &Storage,
    status: &Mutex<Status>,
    cancel: &CancelToken,
    playing: &AtomicBool,
    scan: JobId,
) -> Result<(), String> {
    let history = storage.scan_history().map_err(|e| e.to_string())?;
    let record = history
        .iter()
        .find(|r| r.id == scan)
        .ok_or("Scan no longer exists")?;
    for member in record.groups.iter().flat_map(|g| &g.members) {
        update(status, |s| s.current = member.title.clone());
        let validation = (|| {
            let path = storage
                .resolve_local_file(member.file)
                .map_err(source_error)?;
            analyze::fingerprint(&path, || checkpoint(cancel, playing, status))
                .map_err(|e| e.to_string())
        })();
        let current = match validation {
            Ok(current) => current,
            Err(error) => {
                if !cancel.is_cancelled() {
                    storage.mark_scan_stale(scan).map_err(|e| e.to_string())?;
                }
                return Err(error);
            }
        };
        if member
            .measurement
            .as_ref()
            .is_none_or(|m| m.signature.content_digest != current.content_digest)
        {
            storage.mark_scan_stale(scan).map_err(|e| e.to_string())?;
            return Err("Files changed since analysis; review and rescan before saving".into());
        }
    }
    storage.publish_scan(scan).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::time::Instant;

    fn wait(service: &ScannerService, check: impl Fn(&Status) -> bool) -> Status {
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            let status = service.status();
            if check(&status) {
                return status;
            }
            assert!(Instant::now() < deadline, "Scanner timed out: {status:?}");
            thread::sleep(Duration::from_millis(10));
        }
    }
    fn collection() -> (tempfile::TempDir, Storage) {
        let dir = tempfile::tempdir().unwrap();
        let library = dir.path().join("album");
        for (name, disc) in [("loud-44100", 1), ("quiet-96000", 2), ("gated-48000", 2)] {
            let folder = library.join(format!("CD{disc}"));
            std::fs::create_dir_all(&folder).unwrap();
            let source = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../tests/replaygain/fixtures")
                .join(format!("{name}.flac"));
            std::fs::copy(source, folder.join(format!("{name}.flac"))).unwrap();
        }
        let storage = Storage::open(dir.path().join("app.db")).unwrap();
        let root = parabolica_library::index::add_folder(&storage, &library).unwrap();
        let root = storage
            .library_roots()
            .unwrap()
            .into_iter()
            .find(|r| r.id == root)
            .unwrap();
        parabolica_library::index::scan(&storage, &root, &CancelToken::new(), |_| {}).unwrap();
        (dir, storage)
    }
    #[test]
    fn scan_review_save_restart_reuse_missing_membership_and_non_destructive_files() {
        let (dir, storage) = collection();
        let all = storage.scan_candidates().unwrap();
        assert_eq!(all.len(), 3);
        assert_eq!(all[0].album, all[2].album);
        let before = all
            .iter()
            .map(|c| analyze::fingerprint(&c.path, || Ok(())).unwrap())
            .collect::<Vec<_>>();
        let selected = all.iter().map(|c| c.file).collect();
        let groups =
            super::super::grouping::review(&storage, &all, &selected, ScanMode::Tags).unwrap();
        assert_eq!(groups.len(), 1);
        let status = Mutex::new(Status::default());
        let cancel = CancelToken::new();
        let playing = AtomicBool::new(false);
        run(
            &storage,
            &status,
            &cancel,
            &playing,
            groups.clone(),
            ScanMode::Tags,
            PeakMethod::TruePeak,
        )
        .unwrap();
        let history = storage.scan_history().unwrap();
        assert_eq!(history[0].state, ScanState::Ready);
        let first = history[0].id;
        assert!((history[0].groups[0].gain_db.unwrap() - (-18.0 + 8.104275638582)).abs() < 0.1);
        save(&storage, &status, &cancel, &playing, first).unwrap();
        assert!(storage.scan_history().unwrap()[0].saved);
        let published = storage.published_analysis(all[0].file).unwrap().unwrap();
        assert_eq!(published.scan, first);
        assert!(!published.stale);
        assert_eq!(published.target_lufs, -18.0);
        drop(storage);
        let storage = Storage::open(dir.path().join("app.db")).unwrap();
        assert!(storage.scan_history().unwrap()[0].saved);
        run(
            &storage,
            &status,
            &cancel,
            &playing,
            groups.clone(),
            ScanMode::Tags,
            PeakMethod::TruePeak,
        )
        .unwrap();
        assert!(
            storage.scan_history().unwrap()[0].groups[0]
                .members
                .iter()
                .all(|m| m.reused)
        );
        for (candidate, expected) in all.iter().zip(before) {
            assert_eq!(
                analyze::fingerprint(&candidate.path, || Ok(())).unwrap(),
                expected
            );
        }
        std::fs::remove_file(&all[1].path).unwrap();
        run(
            &storage,
            &status,
            &cancel,
            &playing,
            groups.clone(),
            ScanMode::Tags,
            PeakMethod::TruePeak,
        )
        .unwrap();
        let failed = storage.scan_history().unwrap().remove(0);
        assert_eq!(failed.state, ScanState::Incomplete);
        assert_eq!(failed.groups[0].gain_db, None);
        assert!(storage.publish_scan(failed.id).is_err());
        storage.split_local_group(vec![all[2].file]).unwrap();
        assert!(storage.publish_scan(first).is_err());
        assert!(
            storage
                .published_analysis(all[0].file)
                .unwrap()
                .unwrap()
                .stale
        );
        assert!(
            storage
                .scan_history()
                .unwrap()
                .iter()
                .find(|r| r.id == first)
                .unwrap()
                .groups[0]
                .stale
        );
        assert!(
            storage
                .create_scan(groups, ScanMode::Tags, PeakMethod::TruePeak)
                .is_err()
        );
    }
    #[test]
    fn playback_priority_cancels_promptly_and_a_fresh_run_can_resume() {
        let (_dir, storage) = collection();
        let service = ScannerService::start(storage.clone());
        wait(&service, |s| !s.busy);
        let all = storage.scan_candidates().unwrap();
        let selected = all.iter().map(|c| c.file).collect();
        let groups =
            super::super::grouping::review(&storage, &all, &selected, ScanMode::Tags).unwrap();
        service.set_playing(true);
        service
            .send(Command::Start(
                groups.clone(),
                ScanMode::Tags,
                PeakMethod::Sample,
            ))
            .unwrap();
        let waiting = wait(&service, |s| s.waiting_for_playback);
        assert_eq!(waiting.done, 0);
        assert!(waiting.cancellable);
        service.cancel();
        assert!(!wait(&service, |s| !s.busy).cancellable);
        assert_eq!(
            storage.scan_history().unwrap()[0].state,
            ScanState::Cancelled
        );
        service.set_playing(false);
        service
            .send(Command::Start(groups, ScanMode::Tags, PeakMethod::Sample))
            .unwrap();
        wait(&service, |s| !s.busy);
        assert_eq!(storage.scan_history().unwrap()[0].state, ScanState::Ready);
    }
    #[test]
    fn grouping_modes_keep_disc_membership_and_require_review_of_subsets() {
        let (_dir, storage) = collection();
        let all = storage.scan_candidates().unwrap();
        let selected = all.iter().map(|c| c.file).collect();
        for (mode, expected) in [
            (ScanMode::Tags, 1),
            (ScanMode::Album, 1),
            (ScanMode::Disc, 2),
            (ScanMode::Tracks, 3),
        ] {
            let groups = super::super::grouping::review(&storage, &all, &selected, mode).unwrap();
            assert_eq!(groups.len(), expected);
            for group in groups {
                if mode == ScanMode::Tracks {
                    assert!(group.albums.is_empty());
                } else {
                    assert_eq!(group.albums.len(), 1);
                    assert_eq!(group.albums[0].1.len(), all.len());
                    assert!(
                        all.iter()
                            .all(|candidate| group.albums[0].1.contains(&candidate.file))
                    );
                }
            }
        }
        let subset = HashSet::from([all[0].file, all[2].file]);
        let groups =
            super::super::grouping::review(&storage, &all, &subset, ScanMode::Tags).unwrap();
        assert!(groups[0].warning.contains("Selected 2 of 3"));
        assert!(groups[0].warning.contains("suspected missing tracks"));
        storage.split_local_group(vec![all[2].file]).unwrap();
        let regrouped = storage.scan_candidates().unwrap();
        assert_eq!(
            super::super::grouping::review(&storage, &regrouped, &selected, ScanMode::Tags)
                .unwrap()
                .len(),
            2
        );
        assert!(
            super::super::grouping::review(
                &storage,
                &all,
                &HashSet::from([MediaFileId::generate()]),
                ScanMode::Tags
            )
            .is_err()
        );
    }
    #[test]
    fn cache_rechecks_full_content_even_when_size_and_mtime_match() {
        use std::fs::{File, FileTimes};
        let (_dir, storage) = collection();
        let all = storage.scan_candidates().unwrap();
        let selected = all.iter().map(|c| c.file).collect();
        let groups =
            super::super::grouping::review(&storage, &all, &selected, ScanMode::Tags).unwrap();
        let status = Mutex::new(Status::default());
        let cancel = CancelToken::new();
        let playing = AtomicBool::new(false);
        run(
            &storage,
            &status,
            &cancel,
            &playing,
            groups.clone(),
            ScanMode::Tags,
            PeakMethod::Sample,
        )
        .unwrap();
        let path = &all[0].path;
        let before = std::fs::metadata(path).unwrap();
        let mut bytes = std::fs::read(path).unwrap();
        let position = bytes
            .windows(b"loud-44100".len())
            .position(|w| w == b"loud-44100")
            .unwrap();
        bytes[position + 1] = b'0';
        std::fs::write(path, &bytes).unwrap();
        File::options()
            .write(true)
            .open(path)
            .unwrap()
            .set_times(FileTimes::new().set_modified(before.modified().unwrap()))
            .unwrap();
        run(
            &storage,
            &status,
            &cancel,
            &playing,
            groups,
            ScanMode::Tags,
            PeakMethod::Sample,
        )
        .unwrap();
        let latest = storage.scan_history().unwrap().remove(0);
        assert_eq!(latest.state, ScanState::Ready);
        assert!(!latest.groups[0].members[0].reused);
        assert!(latest.groups[0].members[1..].iter().all(|m| m.reused));
    }
    #[test]
    fn restart_marks_interrupted_runs_cancelled_and_track_scans_ignore_album_regrouping() {
        let (_dir, storage) = collection();
        let all = storage.scan_candidates().unwrap();
        let selected = all.iter().map(|c| c.file).collect();
        let groups =
            super::super::grouping::review(&storage, &all, &selected, ScanMode::Tracks).unwrap();
        let queued = storage
            .create_scan(groups.clone(), ScanMode::Tracks, PeakMethod::Sample)
            .unwrap();
        let running = storage
            .create_scan(groups.clone(), ScanMode::Tracks, PeakMethod::Sample)
            .unwrap();
        storage
            .set_scan_state(running, ScanState::Scanning, None)
            .unwrap();
        let service = ScannerService::start(storage.clone());
        wait(&service, |s| !s.busy);
        let history = storage.scan_history().unwrap();
        for id in [queued, running] {
            assert_eq!(
                history.iter().find(|r| r.id == id).unwrap().state,
                ScanState::Cancelled
            );
        }
        service
            .send(Command::Start(groups, ScanMode::Tracks, PeakMethod::Sample))
            .unwrap();
        wait(&service, |s| !s.busy);
        let completed = storage.scan_history().unwrap().remove(0);
        assert_eq!(completed.state, ScanState::Ready);
        storage.publish_scan(completed.id).unwrap();
        let album_groups =
            super::super::grouping::review(&storage, &all, &selected, ScanMode::Tags).unwrap();
        let unaggregated = storage
            .create_scan(album_groups, ScanMode::Tags, PeakMethod::Sample)
            .unwrap();
        storage
            .set_scan_state(unaggregated, ScanState::Scanning, None)
            .unwrap();
        for member in completed.groups.iter().flat_map(|g| &g.members) {
            storage
                .record_scan_member(
                    unaggregated,
                    member.file,
                    Some(member.measurement.as_ref().unwrap().id),
                    None,
                    true,
                )
                .unwrap();
        }
        assert_eq!(
            storage.finish_scan(unaggregated).unwrap(),
            ScanState::Incomplete
        );
        assert!(storage.publish_scan(unaggregated).is_err());
        storage.split_local_group(vec![all[2].file]).unwrap();
        assert!(
            storage
                .scan_history()
                .unwrap()
                .into_iter()
                .find(|r| r.id == completed.id)
                .unwrap()
                .groups
                .iter()
                .all(|g| !g.stale)
        );
        storage
            .set_media_file_presence(all[0].file, parabolica_storage::library::Presence::Missing)
            .unwrap();
        assert!(
            storage
                .published_analysis(all[0].file)
                .unwrap()
                .unwrap()
                .stale
        );
        assert!(
            !storage
                .published_analysis(all[1].file)
                .unwrap()
                .unwrap()
                .stale
        );
    }
    #[test]
    fn partial_disc_without_declared_total_warns_about_library_members() {
        let (_dir, storage) = collection();
        let all = storage.scan_candidates().unwrap();
        let first = all.iter().find(|c| c.disc == 2 && c.track == 1).unwrap();
        assert!(
            storage
                .media_file_tags(first.file)
                .unwrap()
                .iter()
                .all(|t| !["TRACKTOTAL", "TOTALTRACKS"].contains(&t.field.as_str()))
        );
        let partial = super::super::grouping::review(
            &storage,
            &all,
            &HashSet::from([first.file]),
            ScanMode::Disc,
        )
        .unwrap();
        assert_eq!(partial.len(), 1);
        assert!(
            partial[0]
                .warning
                .contains("Disc 2: selected 1 of 2 library tracks")
        );
        let whole_disc = all.iter().filter(|c| c.disc == 2).map(|c| c.file).collect();
        let complete =
            super::super::grouping::review(&storage, &all, &whole_disc, ScanMode::Disc).unwrap();
        assert!(complete[0].warning.is_empty());
    }

    #[test]
    fn save_time_file_loss_marks_stale_and_cancellation_stays_retryable() {
        let (dir, storage) = collection();
        let all = storage.scan_candidates().unwrap();
        let selected = all.iter().map(|c| c.file).collect();
        let groups =
            super::super::grouping::review(&storage, &all, &selected, ScanMode::Tags).unwrap();
        let status = Mutex::new(Status::default());
        let cancel = CancelToken::new();
        let playing = AtomicBool::new(false);
        run(
            &storage,
            &status,
            &cancel,
            &playing,
            groups.clone(),
            ScanMode::Tags,
            PeakMethod::Sample,
        )
        .unwrap();
        let first = storage.scan_history().unwrap()[0].id;
        let cancelled = CancelToken::new();
        cancelled.cancel();
        assert!(save(&storage, &status, &cancelled, &playing, first).is_err());
        let retryable = storage.scan_history().unwrap().remove(0);
        assert_eq!(retryable.state, ScanState::Ready);
        assert!(!retryable.saved);
        assert!(retryable.groups.iter().all(|g| !g.stale));
        save(&storage, &status, &cancel, &playing, first).unwrap();
        assert!(storage.published_analysis(all[0].file).unwrap().is_some());

        run(
            &storage,
            &status,
            &cancel,
            &playing,
            groups,
            ScanMode::Tags,
            PeakMethod::Sample,
        )
        .unwrap();
        let missing = storage.scan_history().unwrap()[0].id;
        std::fs::remove_file(&all[0].path).unwrap();
        assert!(save(&storage, &status, &cancel, &playing, missing).is_err());
        let invalidated = storage
            .scan_history()
            .unwrap()
            .into_iter()
            .find(|r| r.id == missing)
            .unwrap();
        assert_eq!(invalidated.state, ScanState::Stale);
        assert!(!invalidated.saved);
        assert!(storage.publish_scan(missing).is_err());
        drop(storage);
        let reopened = Storage::open(dir.path().join("app.db")).unwrap();
        assert_eq!(
            reopened
                .scan_history()
                .unwrap()
                .into_iter()
                .find(|r| r.id == missing)
                .unwrap()
                .state,
            ScanState::Stale
        );
    }
}
