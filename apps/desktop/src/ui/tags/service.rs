//! The Files & tags worker. File reads, hashing and writes run on its thread; the page reads a
//! memory-only [`Status`].

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{
    Arc, Mutex,
    mpsc::{self, SyncSender},
};
use std::thread::{self, JoinHandle};

use parabolica_domain::control::CancelToken;
use parabolica_domain::ids::{ArtworkId, JobId, MediaFileId, ReleaseId};
use parabolica_domain::tags::PictureType;
use parabolica_storage::Storage;
use parabolica_storage::tag_writes::{
    NewTagWrite, TagSidecarState, TagTargetState, TagWriteRecord, TagWriteSidecar, TagWriteTarget,
};
use parabolica_tags::artwork::{StagedImage, existing_sidecars};
use parabolica_tags::job::{self, Ledger, SidecarPlan, SidecarState, TargetPlan, TargetState};
use parabolica_tags::write::{self, NoHooks};

use super::review::{AlbumReview, FileState, Folder, PICTURE_TYPES, Plan, ReviewFile};

#[derive(Debug, Clone)]
pub struct Staged {
    pub id: ArtworkId,
    pub kind: PictureType,
    pub name: String,
    pub image: StagedImage,
}

#[derive(Debug, Clone, Default)]
pub struct Status {
    pub revision: u64,
    pub busy: bool,
    pub cancellable: bool,
    pub operation: String,
    /// Finished files and sidecars of the running write, and their total.
    pub progress: Option<(usize, usize)>,
    pub review: Option<Arc<AlbumReview>>,
    pub staged: Arc<Vec<Staged>>,
    pub history: Arc<Vec<TagWriteRecord>>,
    /// Counts finished writes, so the page knows when to refresh the library.
    pub writes_finished: u64,
    pub error: Option<String>,
}

#[derive(Debug)]
pub enum Command {
    Load(ReleaseId),
    Stage(PathBuf, PictureType),
    Unstage(ArtworkId),
    Apply(Box<Plan>),
    Retry(JobId),
    Restore(JobId, MediaFileId),
    DiscardBackups(JobId),
    Shutdown,
}

#[derive(Debug)]
pub struct TagService {
    sender: SyncSender<Command>,
    status: Arc<Mutex<Status>>,
    cancel: Arc<Mutex<CancelToken>>,
    in_use: Arc<Mutex<Option<PathBuf>>>,
    thread: Option<JoinHandle<()>>,
}

struct Worker {
    storage: Storage,
    status: Arc<Mutex<Status>>,
    in_use: Arc<Mutex<Option<PathBuf>>>,
    backups: PathBuf,
    staging: PathBuf,
    album: Option<ReleaseId>,
    staged: HashMap<ArtworkId, (StagedImage, PathBuf)>,
}

impl TagService {
    /// Backups and staged images live beside the database.
    pub fn start(storage: Storage) -> Self {
        let data = storage
            .path()
            .parent()
            .map(Path::to_owned)
            .unwrap_or_default();
        let (sender, receiver) = mpsc::sync_channel(8);
        let status = Arc::new(Mutex::new(Status::default()));
        let cancel = Arc::new(Mutex::new(CancelToken::new()));
        let in_use = Arc::new(Mutex::new(None));
        let mut worker = Worker {
            storage,
            status: status.clone(),
            in_use: in_use.clone(),
            backups: data.join("tag-backups"),
            staging: data.join("tag-staging"),
            album: None,
            staged: HashMap::new(),
        };
        let worker_cancel = cancel.clone();
        let thread = thread::spawn(move || {
            if let Err(error) = worker.recover() {
                worker.update(|s| s.error = Some(error));
            }
            while let Ok(command) = receiver.recv() {
                if matches!(command, Command::Shutdown) {
                    break;
                }
                let token = lock(&worker_cancel).clone();
                let result = worker.handle(command, &token);
                worker.update(|s| {
                    if let Err(error) = result {
                        s.error = Some(error);
                    }
                    s.busy = false;
                    s.cancellable = false;
                    s.progress = None;
                    s.operation.clear();
                });
            }
        });
        Self {
            sender,
            status,
            cancel,
            in_use,
            thread: Some(thread),
        }
    }

    pub fn send(&self, command: Command) -> Result<(), String> {
        let mut status = lock(&self.status);
        if status.busy {
            return Err("Files & tags is busy".into());
        }
        *lock(&self.cancel) = CancelToken::new();
        status.operation = match &command {
            Command::Load(_) => "Reading files",
            Command::Stage(..) => "Checking image",
            Command::Unstage(_) => "Removing image",
            Command::Apply(_) | Command::Retry(_) => "Writing",
            Command::Restore(..) => "Restoring",
            Command::DiscardBackups(_) => "Deleting backups",
            Command::Shutdown => "Closing",
        }
        .into();
        status.busy = true;
        status.cancellable = matches!(&command, Command::Apply(_) | Command::Retry(_));
        status.error = None;
        status.revision += 1;
        if self.sender.try_send(command).is_err() {
            status.busy = false;
            status.cancellable = false;
            return Err("Files & tags command queue is full".into());
        }
        Ok(())
    }

    pub fn cancel(&self) {
        lock(&self.cancel).cancel();
    }

    /// The file the player has open. It is deferred rather than replaced.
    pub fn set_in_use(&self, path: Option<PathBuf>) {
        *lock(&self.in_use) = path;
    }

    pub fn status(&self) -> Status {
        lock(&self.status).clone()
    }
}

impl Drop for TagService {
    fn drop(&mut self) {
        self.cancel();
        let _ = self.sender.send(Command::Shutdown);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}

impl Worker {
    fn update(&self, f: impl FnOnce(&mut Status)) {
        let mut status = lock(&self.status);
        f(&mut status);
        status.revision += 1;
    }

    fn handle(&mut self, command: Command, cancel: &CancelToken) -> Result<(), String> {
        match command {
            Command::Load(album) => self.load(album),
            Command::Stage(path, kind) => self.stage(&path, kind),
            Command::Unstage(id) => {
                if let Some((_, path)) = self.staged.remove(&id) {
                    let _ = std::fs::remove_file(path);
                }
                self.publish_staged();
                Ok(())
            }
            Command::Apply(plan) => self.apply(*plan, cancel),
            Command::Retry(job) => self.retry(job, cancel),
            Command::Restore(job, file) => self.restore(job, file),
            Command::DiscardBackups(job) => self.discard(job),
            Command::Shutdown => Ok(()),
        }
    }

    fn load(&mut self, album: ReleaseId) -> Result<(), String> {
        let previous: Vec<MediaFileId> = lock(&self.status)
            .review
            .as_ref()
            .filter(|review| review.album == album)
            .map(|review| review.files.iter().map(|f| f.file).collect())
            .unwrap_or_default();
        let mut album = album;
        let mut members = self
            .storage
            .album_edit_files(album)
            .map_err(|e| e.to_string())?;
        if members.is_empty() {
            for file in previous {
                if let Some(moved) = self
                    .storage
                    .album_of_file(file)
                    .map_err(|e| e.to_string())?
                {
                    album = moved;
                    members = self
                        .storage
                        .album_edit_files(album)
                        .map_err(|e| e.to_string())?;
                    break;
                }
            }
        }
        if self.album != Some(album) {
            self.album = Some(album);
            // Images staged for another album were never written; no job refers to them.
            for (_, path) in self.staged.drain().map(|(_, staged)| staged) {
                let _ = std::fs::remove_file(path);
            }
            self.update(|s| {
                s.review = None;
                s.staged = Arc::default();
            });
        }
        let mut files = Vec::new();
        let mut folders: Vec<PathBuf> = Vec::new();
        for member in members {
            let state = match &member.path {
                None => FileState::Unavailable("Missing or offline".into()),
                Some(path) if !parabolica_tags::writable_format(path) => {
                    FileState::Unavailable(format!(
                        "{} files can't be written in this build",
                        path.extension()
                            .map(|e| e.to_string_lossy().to_uppercase())
                            .unwrap_or_else(|| member.format.clone())
                    ))
                }
                Some(path) => match write::read_tags(path)
                    .and_then(|tags| Ok((tags, write::fingerprint(path)?)))
                {
                    Ok((tags, reviewed)) => {
                        if let Some(dir) = path.parent()
                            && !folders.iter().any(|f| f == dir)
                        {
                            folders.push(dir.to_owned());
                        }
                        FileState::Ready {
                            tags: Box::new(tags),
                            reviewed,
                        }
                    }
                    Err(error) => FileState::Unavailable(error.to_string()),
                },
            };
            files.push(ReviewFile {
                file: member.file,
                path: member.path,
                disc: member.disc,
                track: member.track,
                title: member.title,
                state,
            });
        }
        let mut reviewed_folders = Vec::new();
        for path in folders {
            let mut existing = Vec::new();
            for kind in PICTURE_TYPES {
                existing.push((
                    kind,
                    existing_sidecars(&path, kind)
                        .map_err(|e| format!("{}: {e}", path.display()))?,
                ));
            }
            reviewed_folders.push(Folder { path, existing });
        }
        let history = self.storage.tag_writes(album).map_err(|e| e.to_string())?;
        self.update(|s| {
            s.review = Some(Arc::new(AlbumReview {
                album,
                files,
                folders: reviewed_folders,
            }));
            s.history = Arc::new(history);
        });
        Ok(())
    }

    fn stage(&mut self, path: &Path, kind: PictureType) -> Result<(), String> {
        let image = StagedImage::read(path).map_err(|e| e.to_string())?;
        let id = ArtworkId::generate();
        std::fs::create_dir_all(&self.staging).map_err(|e| e.to_string())?;
        let staged = self
            .staging
            .join(format!("{id}.{}", image.kind().extension()));
        std::fs::write(&staged, image.bytes()).map_err(|e| e.to_string())?;
        if kind != PictureType::Booklet {
            let replaced: Vec<ArtworkId> = lock(&self.status)
                .staged
                .iter()
                .filter(|s| s.kind == kind)
                .map(|s| s.id)
                .collect();
            for old in replaced {
                if let Some((_, path)) = self.staged.remove(&old) {
                    let _ = std::fs::remove_file(path);
                }
            }
        }
        self.staged.insert(id, (image.clone(), staged));
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        self.update(|s| {
            let mut list: Vec<Staged> = s
                .staged
                .iter()
                .filter(|s| kind == PictureType::Booklet || s.kind != kind)
                .cloned()
                .collect();
            list.push(Staged {
                id,
                kind,
                name,
                image,
            });
            s.staged = Arc::new(list);
        });
        Ok(())
    }

    fn publish_staged(&self) {
        let keep: HashSet<ArtworkId> = self.staged.keys().copied().collect();
        self.update(|s| {
            s.staged = Arc::new(
                s.staged
                    .iter()
                    .filter(|s| keep.contains(&s.id))
                    .cloned()
                    .collect(),
            );
        });
    }

    fn apply(&mut self, plan: Plan, cancel: &CancelToken) -> Result<(), String> {
        let mut images = HashMap::new();
        let mut staged_paths = Vec::new();
        let used: HashSet<ArtworkId> = plan
            .targets
            .iter()
            .flat_map(|t| t.patch.artwork.iter().map(|a| a.artwork))
            .collect();
        for id in used {
            let (image, path) = self
                .staged
                .get(&id)
                .ok_or("A selected image is no longer staged; choose it again")?;
            images.insert(id, image.clone());
            staged_paths.push((id, path.clone()));
        }
        let job = self
            .storage
            .create_tag_write(NewTagWrite {
                album: plan.album,
                retry_of: None,
                targets: plan.targets.iter().map(target_to_storage).collect(),
                images: staged_paths,
                sidecars: plan.sidecars.iter().map(sidecar_to_storage).collect(),
            })
            .map_err(|e| e.to_string())?;
        // The job owns its images now; a later choice must not delete a file a retry needs.
        for id in images.keys() {
            self.staged.remove(id);
        }
        self.publish_staged();
        self.execute(job, &plan.targets, &plan.sidecars, &images, cancel)?;
        self.load(plan.album)
    }

    fn retry(&mut self, previous: JobId, cancel: &CancelToken) -> Result<(), String> {
        let record = self
            .storage
            .tag_write(previous)
            .map_err(|e| e.to_string())?
            .ok_or("Write not found")?;
        let targets: Vec<TargetPlan> = record
            .targets
            .iter()
            .filter(|(_, state)| target_from_storage(state).retryable())
            .map(|(target, _)| TargetPlan {
                patch: target.patch.clone(),
                path: target.path.clone(),
            })
            .collect();
        let sidecars: Vec<SidecarPlan> = record
            .sidecars
            .iter()
            .filter(|(_, state)| sidecar_from_storage(state).retryable())
            .map(|(sidecar, _)| SidecarPlan {
                directory: sidecar.directory.clone(),
                file_name: sidecar.file_name.clone(),
                picture_type: sidecar.picture_type,
                artwork: sidecar.artwork,
                replace: sidecar.replace.clone(),
            })
            .collect();
        if targets.is_empty() && sidecars.is_empty() {
            return Err("Nothing to retry".into());
        }
        let mut images = HashMap::new();
        for (id, path) in &record.images {
            let image = StagedImage::read(path)
                .map_err(|e| format!("Staged image unavailable ({e}); choose it again"))?;
            images.insert(*id, image);
        }
        let job = self
            .storage
            .create_tag_write(NewTagWrite {
                album: record.album,
                retry_of: Some(previous),
                targets: targets.iter().map(target_to_storage).collect(),
                images: record.images.clone(),
                sidecars: sidecars.iter().map(sidecar_to_storage).collect(),
            })
            .map_err(|e| e.to_string())?;
        self.execute(job, &targets, &sidecars, &images, cancel)?;
        self.load(record.album)
    }

    fn execute(
        &mut self,
        id: JobId,
        targets: &[TargetPlan],
        sidecars: &[SidecarPlan],
        images: &HashMap<ArtworkId, StagedImage>,
        cancel: &CancelToken,
    ) -> Result<(), String> {
        self.storage
            .start_tag_write(id)
            .map_err(|e| e.to_string())?;
        let total = targets.len() + sidecars.len();
        self.update(|s| s.progress = Some((0, total)));
        let in_use = self.in_use.clone();
        let mut ledger = StorageLedger {
            storage: &self.storage,
            status: &self.status,
            job: id,
            done: 0,
        };
        let result = job::run(
            job::Job {
                id,
                targets,
                sidecars,
                images,
                backup_dir: &self.backups,
            },
            &mut ledger,
            &|path| lock(&in_use).as_deref() == Some(path),
            cancel,
            &NoHooks,
        );
        let finished = self
            .storage
            .finish_tag_write(id, matches!(result, Ok(false)))
            .map_err(|e| e.to_string());
        self.update(|s| s.writes_finished += 1);
        result?;
        finished.map(|_| ())
    }

    fn restore(&mut self, id: JobId, file: MediaFileId) -> Result<(), String> {
        let record = self
            .storage
            .tag_write(id)
            .map_err(|e| e.to_string())?
            .ok_or("Write not found")?;
        if record.backups_discarded {
            return Err("Backups were deleted".into());
        }
        let (target, state) = record
            .targets
            .iter()
            .find(|(t, _)| t.patch.target == file)
            .ok_or("File not found in this write")?;
        let TagTargetState::Written { signature, backup } = state else {
            return Err("This file has no written content to restore".into());
        };
        if lock(&self.in_use).as_deref() == Some(target.path.as_path()) {
            return Err("Stop playback of this file before restoring it".into());
        }
        let temp = job::temp_path(&target.path, id).with_extension("restore");
        let restored =
            write::restore(&target.path, backup, signature, &temp).map_err(|e| match e {
                write::WriteError::Changed => {
                    "The file changed after the write; it was not restored".to_owned()
                }
                other => other.to_string(),
            })?;
        self.storage
            .record_tag_restore(id, file, restored)
            .map_err(|e| e.to_string())?;
        self.update(|s| s.writes_finished += 1);
        self.load(record.album)
    }

    fn discard(&mut self, id: JobId) -> Result<(), String> {
        let record = self
            .storage
            .tag_write(id)
            .map_err(|e| e.to_string())?
            .ok_or("Write not found")?;
        let dir = job::job_backup_dir(&self.backups, id);
        match std::fs::remove_dir_all(&dir) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(format!("{}: {e}", dir.display())),
        }
        self.storage
            .discard_tag_backups(id)
            .map_err(|e| e.to_string())?;
        // Staged images still referenced by a write that keeps its backups stay for retries.
        let history = self
            .storage
            .tag_writes(record.album)
            .map_err(|e| e.to_string())?;
        let referenced: HashSet<&PathBuf> = history
            .iter()
            .filter(|r| !r.backups_discarded)
            .flat_map(|r| r.images.iter().map(|(_, path)| path))
            .collect();
        for (_, path) in &record.images {
            if !referenced.contains(path) && !self.staged.values().any(|(_, p)| p == path) {
                let _ = std::fs::remove_file(path);
            }
        }
        self.load(record.album)
    }

    /// Resolve writes interrupted by a crash or forced exit from the files' current content.
    fn recover(&mut self) -> Result<(), String> {
        let unfinished = self
            .storage
            .unfinished_tag_writes()
            .map_err(|e| e.to_string())?;
        for record in unfinished {
            if matches!(record.state, parabolica_domain::jobs::JobState::Running(_)) {
                for (target, state) in &record.targets {
                    let state = target_from_storage(state);
                    if matches!(state, TargetState::Pending | TargetState::Replacing { .. }) {
                        let resolved =
                            job::recover(record.id, &target.path, &target.patch.reviewed, &state);
                        self.storage
                            .record_tag_target(
                                record.id,
                                target.patch.target,
                                target_to_storage_state(&resolved),
                            )
                            .map_err(|e| e.to_string())?;
                    }
                }
                for (index, (sidecar, state)) in record.sidecars.iter().enumerate() {
                    let state = sidecar_from_storage(state);
                    if state == SidecarState::Pending {
                        let resolved = job::recover_sidecar(
                            record.id,
                            &SidecarPlan {
                                directory: sidecar.directory.clone(),
                                file_name: sidecar.file_name.clone(),
                                picture_type: sidecar.picture_type,
                                artwork: sidecar.artwork,
                                replace: sidecar.replace.clone(),
                            },
                            &state,
                        );
                        self.storage
                            .record_tag_sidecar(
                                record.id,
                                index,
                                sidecar_to_storage_state(&resolved),
                            )
                            .map_err(|e| e.to_string())?;
                    }
                }
            }
            self.storage
                .finish_tag_write(record.id, false)
                .map_err(|e| e.to_string())?;
        }
        Ok(())
    }
}

struct StorageLedger<'a> {
    storage: &'a Storage,
    status: &'a Mutex<Status>,
    job: JobId,
    done: usize,
}

impl StorageLedger<'_> {
    fn advance(&mut self) {
        self.done += 1;
        let mut status = lock(self.status);
        if let Some((done, _)) = &mut status.progress {
            *done = self.done;
        }
        status.revision += 1;
    }
}

impl Ledger for StorageLedger<'_> {
    fn target(&mut self, file: MediaFileId, state: &TargetState) -> Result<(), String> {
        self.storage
            .record_tag_target(self.job, file, target_to_storage_state(state))
            .map_err(|e| e.to_string())?;
        if !matches!(state, TargetState::Replacing { .. }) {
            self.advance();
        }
        Ok(())
    }

    fn sidecar(&mut self, index: usize, state: &SidecarState) -> Result<(), String> {
        self.storage
            .record_tag_sidecar(self.job, index, sidecar_to_storage_state(state))
            .map_err(|e| e.to_string())?;
        self.advance();
        Ok(())
    }
}

fn target_to_storage(target: &TargetPlan) -> TagWriteTarget {
    TagWriteTarget {
        patch: target.patch.clone(),
        path: target.path.clone(),
    }
}

fn sidecar_to_storage(sidecar: &SidecarPlan) -> TagWriteSidecar {
    TagWriteSidecar {
        directory: sidecar.directory.clone(),
        file_name: sidecar.file_name.clone(),
        picture_type: sidecar.picture_type,
        artwork: sidecar.artwork,
        replace: sidecar.replace.clone(),
    }
}

pub(crate) fn target_to_storage_state(state: &TargetState) -> TagTargetState {
    match state.clone() {
        TargetState::Pending => TagTargetState::Pending,
        TargetState::Replacing { backup, expected } => {
            TagTargetState::Replacing { backup, expected }
        }
        TargetState::Written { signature, backup } => TagTargetState::Written { signature, backup },
        TargetState::Unchanged => TagTargetState::Unchanged,
        TargetState::Changed => TagTargetState::Changed,
        TargetState::Deferred { reason } => TagTargetState::Deferred { reason },
        TargetState::Failed { reason } => TagTargetState::Failed { reason },
        TargetState::Restored { signature } => TagTargetState::Restored { signature },
    }
}

pub(crate) fn target_from_storage(state: &TagTargetState) -> TargetState {
    match state.clone() {
        TagTargetState::Pending => TargetState::Pending,
        TagTargetState::Replacing { backup, expected } => {
            TargetState::Replacing { backup, expected }
        }
        TagTargetState::Written { signature, backup } => TargetState::Written { signature, backup },
        TagTargetState::Unchanged => TargetState::Unchanged,
        TagTargetState::Changed => TargetState::Changed,
        TagTargetState::Deferred { reason } => TargetState::Deferred { reason },
        TagTargetState::Failed { reason } => TargetState::Failed { reason },
        TagTargetState::Restored { signature } => TargetState::Restored { signature },
    }
}

fn sidecar_to_storage_state(state: &SidecarState) -> TagSidecarState {
    match state.clone() {
        SidecarState::Pending => TagSidecarState::Pending,
        SidecarState::Written => TagSidecarState::Written,
        SidecarState::Changed => TagSidecarState::Changed,
        SidecarState::Failed { reason } => TagSidecarState::Failed { reason },
    }
}

pub(crate) fn sidecar_from_storage(state: &TagSidecarState) -> SidecarState {
    match state.clone() {
        TagSidecarState::Pending => SidecarState::Pending,
        TagSidecarState::Written => SidecarState::Written,
        TagSidecarState::Changed => SidecarState::Changed,
        TagSidecarState::Failed { reason } => SidecarState::Failed { reason },
    }
}
