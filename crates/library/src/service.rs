//! One bounded indexing worker. Watch events coalesce into reconciliation requests.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{
    Arc, Mutex,
    mpsc::{self, Receiver, SyncSender, TrySendError},
};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use parabolica_domain::control::CancelToken;
use parabolica_domain::ids::{LibraryRootId, MediaFileId, ReleaseId};
use parabolica_domain::jobs::{JobKind, JobState, Progress};
use parabolica_storage::{Storage, library::LibraryRoot};

use crate::index::{self, LibraryError, ScanProgress};

const RECONCILE: Duration = Duration::from_secs(30);
const DEBOUNCE: Duration = Duration::from_millis(500);
#[derive(Debug)]
pub enum Command {
    Add(PathBuf),
    Remove(LibraryRootId),
    Relocate(LibraryRootId, PathBuf),
    Recover(MediaFileId, PathBuf),
    Rescan,
    Group(Vec<MediaFileId>, ReleaseId),
    ResetGroup(Vec<MediaFileId>),
    SplitGroup(Vec<MediaFileId>),
}
#[derive(Debug, Clone, Default)]
pub struct LibraryStatus {
    pub revision: u64,
    pub scanning: bool,
    pub progress: ScanProgress,
    pub error: Option<String>,
    pub command_error: Option<String>,
    pub failures: Vec<String>,
    pub watcher_error: Option<String>,
}
#[derive(Debug)]
pub struct LibraryService {
    commands: Option<SyncSender<Command>>,
    state: Arc<Mutex<LibraryStatus>>,
    cancel: Arc<Mutex<CancelToken>>,
    stopping: Arc<AtomicBool>,
    playing: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}
impl LibraryService {
    pub fn start(storage: Storage) -> Self {
        let (commands, receiver) = mpsc::sync_channel(32);
        let state = Arc::new(Mutex::new(LibraryStatus::default()));
        let cancel = Arc::new(Mutex::new(CancelToken::new()));
        let stopping = Arc::new(AtomicBool::new(false));
        let playing = Arc::new(AtomicBool::new(false));
        let (s, c, stop, p) = (
            state.clone(),
            cancel.clone(),
            stopping.clone(),
            playing.clone(),
        );
        let worker = thread::spawn(move || run(storage, receiver, s, c, stop, p));
        Self {
            commands: Some(commands),
            state,
            cancel,
            stopping,
            playing,
            worker: Some(worker),
        }
    }
    pub fn send(&self, command: Command) -> Result<(), String> {
        let current = self.cancel.lock().unwrap();
        self.commands
            .as_ref()
            .ok_or("Indexing worker stopped")?
            .try_send(command)
            .map_err(|error| match error {
                TrySendError::Full(_) => "Folder changes are busy. Try again.".to_owned(),
                TrySendError::Disconnected(_) => "Indexing worker stopped".to_owned(),
            })?;
        current.cancel();
        Ok(())
    }

    pub fn status(&self) -> LibraryStatus {
        self.state.lock().unwrap().clone()
    }
    pub fn cancel(&self) {
        self.cancel.lock().unwrap().cancel();
    }
    pub fn set_playing(&self, playing: bool) {
        self.playing.store(playing, Ordering::Relaxed);
    }
}
impl Drop for LibraryService {
    fn drop(&mut self) {
        self.stopping.store(true, Ordering::Release);
        self.cancel();
        self.commands.take();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
fn changed(state: &Mutex<LibraryStatus>, edit: impl FnOnce(&mut LibraryStatus)) {
    let mut s = state.lock().unwrap();
    edit(&mut s);
    s.revision += 1;
}
fn run(
    storage: Storage,
    commands: Receiver<Command>,
    state: Arc<Mutex<LibraryStatus>>,
    cancel: Arc<Mutex<CancelToken>>,
    stopping: Arc<AtomicBool>,
    playing: Arc<AtomicBool>,
) {
    if let Ok(jobs) = storage.unfinished_jobs() {
        for job in jobs
            .into_iter()
            .filter(|job| job.kind == JobKind::LibraryIndex)
        {
            if let Err(error) = storage.transition_job(job.id, JobState::Cancelled) {
                changed(&state, |s| s.error = Some(error.to_string()));
            }
        }
    }
    let dirty = Arc::new(AtomicBool::new(false));
    let callback_dirty = dirty.clone();
    let (watch_errors, watch_rx) = mpsc::sync_channel(1);
    let mut watcher =
        match notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
            watch_event(event, &callback_dirty, &watch_errors);
        }) {
            Ok(w) => Some(w),
            Err(e) => {
                changed(&state, |s| {
                    s.watcher_error = Some(format!(
                        "Folder watcher unavailable: {e}. Retrying every 30 seconds."
                    ))
                });
                None
            }
        };
    let mut watched = HashMap::new();
    let mut pending = true;
    let mut due = Instant::now();
    let mut pending_since = due;
    let mut last_scan = Instant::now();
    let mut paused = false;
    while !stopping.load(Ordering::Acquire) {
        match commands.recv_timeout(Duration::from_millis(100)) {
            Ok(command) => {
                let result = apply(&storage, command);
                pending = true;
                due = Instant::now();
                paused = false;
                changed(&state, |s| {
                    s.command_error = result.err().map(|error| error.to_string())
                });
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
        if let Ok(error) = watch_rx.try_recv() {
            changed(&state, |s| {
                s.watcher_error = Some(format!("Folder watcher: {error}. Reconciling folders."))
            });
        }
        if dirty.swap(false, Ordering::AcqRel) && !paused {
            if !pending {
                pending_since = Instant::now();
            }
            pending = true;
            due = (Instant::now() + DEBOUNCE).min(pending_since + Duration::from_secs(2));
        }
        if !paused && last_scan.elapsed() >= RECONCILE && !pending {
            pending = true;
            due = Instant::now();
        }
        if !pending || Instant::now() < due {
            continue;
        }
        pending = false;
        last_scan = Instant::now();
        let roots = match storage.library_roots() {
            Ok(r) => r,
            Err(e) => {
                changed(&state, |s| s.error = Some(e.to_string()));
                continue;
            }
        };
        refresh_watches(&mut watcher, &roots, &mut watched, &state);
        let token = CancelToken::new();
        *cancel.lock().unwrap() = token.clone();
        changed(&state, |s| {
            s.scanning = true;
            s.progress = ScanProgress::default();
            s.failures.clear();
            s.error = None;
        });
        for root in roots {
            if token.is_cancelled() || stopping.load(Ordering::Acquire) {
                break;
            }
            let mut last_publish = Instant::now() - Duration::from_secs(1);
            let job = match storage
                .create_job(JobKind::LibraryIndex, Vec::new())
                .and_then(|job| {
                    storage.transition_job(
                        job,
                        JobState::Running(Progress {
                            done: 0,
                            total: None,
                        }),
                    )?;
                    Ok(job)
                }) {
                Ok(job) => job,
                Err(error) => {
                    changed(&state, |s| s.error = Some(error.to_string()));
                    continue;
                }
            };
            let result = index::scan(&storage, &root, &token, |progress| {
                if last_publish.elapsed() >= Duration::from_millis(150)
                    || progress.finished
                    || progress.cancelled
                {
                    changed(&state, |s| publish_progress(s, progress));
                    last_publish = Instant::now();
                }
                if playing.load(Ordering::Relaxed) {
                    thread::sleep(Duration::from_millis(2));
                }
            });
            let outcome = match &result {
                Err(LibraryError::Cancelled) => JobState::Cancelled,
                Err(error) => JobState::Failed {
                    reason: error.to_string(),
                },
                Ok(progress) if progress.finished && progress.errors == 0 => JobState::Completed,
                Ok(_) => JobState::Incomplete,
            };
            if let Ok(progress) = &result
                && let Err(error) = storage.transition_job(
                    job,
                    JobState::Running(Progress {
                        done: progress.visited as u64,
                        total: Some(progress.visited as u64),
                    }),
                )
            {
                changed(&state, |s| s.error = Some(error.to_string()));
            }
            if let Err(error) = storage.transition_job(job, outcome) {
                changed(&state, |s| s.error = Some(error.to_string()));
            }
            match result {
                Err(LibraryError::Cancelled) => break,
                Err(error) => changed(&state, |s| {
                    s.error = Some(format!("{}: {error}", root.path.display()))
                }),
                Ok(progress) => changed(&state, |s| publish_progress(s, &progress)),
            }
        }
        if token.is_cancelled() {
            paused = true;
        }
        changed(&state, |s| {
            s.scanning = false;
            s.progress.cancelled = token.is_cancelled();
        });
    }
}
fn publish_progress(state: &mut LibraryStatus, progress: &ScanProgress) {
    state.progress = progress.clone();
    for failure in &progress.failures {
        if state.failures.len() < 32 && !state.failures.contains(failure) {
            state.failures.push(failure.clone());
        }
    }
}

fn refresh_watches(
    watcher: &mut Option<RecommendedWatcher>,
    roots: &[LibraryRoot],
    watched: &mut HashMap<LibraryRootId, PathBuf>,
    state: &Mutex<LibraryStatus>,
) {
    let Some(watcher) = watcher else {
        return;
    };
    watched.retain(|id, path| {
        let keep = roots.iter().any(|r| r.id == *id && r.path == *path) && path.is_dir();
        if !keep {
            let _ = watcher.unwatch(path);
        }
        keep
    });
    for root in roots {
        if watched.contains_key(&root.id) || !root.path.is_dir() {
            continue;
        }
        match watcher.watch(&root.path, RecursiveMode::Recursive) {
            Ok(()) => {
                watched.insert(root.id, root.path.clone());
            }
            Err(e) => changed(state, |s| {
                s.watcher_error = Some(format!(
                    "Can't watch {}: {e}. Retrying every 30 seconds.",
                    root.path.display()
                ))
            }),
        }
    }
}
fn apply(storage: &Storage, command: Command) -> Result<(), LibraryError> {
    match command {
        Command::Add(path) => {
            index::add_folder(storage, &path)?;
        }
        Command::Remove(root) => storage.remove_library_root(root)?,
        Command::Relocate(root, path) => index::relocate_folder(storage, root, &path)?,
        Command::Recover(file, path) => index::recover_file(storage, file, &path)?,
        Command::Group(files, album) => storage.group_local_files(files, album)?,
        Command::ResetGroup(files) => storage.reset_local_grouping(files)?,
        Command::SplitGroup(files) => {
            storage.split_local_group(files)?;
        }
        Command::Rescan => {}
    }
    Ok(())
}

fn watch_event(
    event: notify::Result<notify::Event>,
    dirty: &AtomicBool,
    errors: &SyncSender<String>,
) {
    match event {
        Ok(event) if event.need_rescan() || !matches!(event.kind, notify::EventKind::Access(_)) => {
            dirty.store(true, Ordering::Release);
        }
        Err(error) => {
            dirty.store(true, Ordering::Release);
            let _ = errors.try_send(error.to_string());
        }
        _ => {}
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn overflow_flags_and_watcher_errors_request_full_reconciliation() {
        let dirty = AtomicBool::new(false);
        let (errors, receiver) = mpsc::sync_channel(1);
        let event = notify::Event::new(notify::EventKind::Access(notify::event::AccessKind::Any))
            .set_flag(notify::event::Flag::Rescan);
        watch_event(Ok(event), &dirty, &errors);
        assert!(dirty.swap(false, Ordering::AcqRel));
        watch_event(Err(notify::Error::generic("overflow")), &dirty, &errors);
        assert!(dirty.load(Ordering::Acquire));
        assert!(receiver.try_recv().is_ok());
    }
}
