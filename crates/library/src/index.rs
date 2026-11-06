//! Incremental reconciliation. A cancelled or incomplete traversal never marks unseen files missing.

use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use parabolica_domain::control::CancelToken;
use parabolica_domain::files::{
    ContentDigest, FileLocation, FileSignature, OsFileId, RootAvailability, VolumeId,
};
use parabolica_domain::ids::{LibraryRootId, MediaFileId};
use parabolica_storage::Storage;
use parabolica_storage::collection::IndexedFile;
use parabolica_storage::library::{LibraryRoot, MediaFileRecord, Presence};
use sha2::{Digest, Sha256};

use crate::metadata::{self, Metadata};

#[derive(Debug, thiserror::Error)]
pub enum LibraryError {
    #[error("{0}")]
    Storage(#[from] parabolica_storage::StorageError),
    #[error("{0}")]
    Io(#[from] io::Error),
    #[error("Indexing cancelled")]
    Cancelled,
    #[error("{0}")]
    Invalid(String),
}
#[derive(Debug, Clone, Default)]
pub struct ScanProgress {
    pub root: Option<LibraryRootId>,
    pub visited: usize,
    pub indexed: usize,
    pub unchanged: usize,
    pub errors: usize,
    pub current: PathBuf,
    pub finished: bool,
    pub cancelled: bool,
    pub failures: Vec<String>,
}

pub fn add_folder(storage: &Storage, path: &Path) -> Result<LibraryRootId, LibraryError> {
    let path = fs::canonicalize(path)?;
    if !path.is_dir() {
        return Err(LibraryError::Invalid("Choose a music folder".into()));
    }
    for root in storage.library_roots()? {
        if metadata::path_key(&root.path) == metadata::path_key(&path) {
            return Ok(root.id);
        }
        if metadata::path_key(&path).starts_with(metadata::path_key(&root.path))
            || metadata::path_key(&root.path).starts_with(metadata::path_key(&path))
        {
            return Err(LibraryError::Invalid(
                "This folder overlaps an existing music folder".into(),
            ));
        }
    }
    let volume = os_identity(&path).map(|id| id.volume);
    Ok(storage.add_library_root(&path, volume)?)
}

pub fn scan(
    storage: &Storage,
    root: &LibraryRoot,
    cancel: &CancelToken,
    mut publish: impl FnMut(&ScanProgress),
) -> Result<ScanProgress, LibraryError> {
    let mut progress = ScanProgress {
        root: Some(root.id),
        ..Default::default()
    };
    let initial_id = os_identity(&root.path);
    let first = match fs::read_dir(&root.path) {
        Ok(entries) => entries,
        Err(error) => {
            storage.set_root_availability(root.id, availability(&error))?;
            return Err(error.into());
        }
    };
    if let (Some(expected), Some(actual)) = (&root.volume, initial_id.as_ref().map(|id| &id.volume))
        && expected != actual
    {
        storage.set_root_availability(root.id, RootAvailability::Offline)?;
        return Err(LibraryError::Invalid("A different drive is mounted at this folder. Use Locate folder to confirm its music files.".into()));
    }
    storage.set_root_availability(root.id, RootAvailability::Online)?;
    let known = storage.indexed_files(root.id)?;
    let by_path: HashMap<_, _> = known
        .iter()
        .map(|f| (metadata::path_key(&f.file.location.relative_path), f))
        .collect();
    let roots = storage
        .library_roots()?
        .into_iter()
        .map(|root| (root.id, root))
        .collect::<HashMap<_, _>>();
    let mut seen = HashSet::new();
    let mut complete = true;
    let mut stack = vec![first];
    while let Some(entries) = stack.last_mut() {
        if cancel.is_cancelled() {
            progress.cancelled = true;
            publish(&progress);
            return Err(LibraryError::Cancelled);
        }
        let entry = match entries.next() {
            Some(Ok(entry)) => entry,
            Some(Err(error)) => {
                complete = false;
                failure(&mut progress, error.to_string());
                continue;
            }
            None => {
                stack.pop();
                continue;
            }
        };
        let path = entry.path();
        let file_meta = match fs::symlink_metadata(&path) {
            Ok(m) => m,
            Err(e) => {
                complete = false;
                failure(&mut progress, format!("{}: {e}", path.display()));
                continue;
            }
        };
        if is_link(&file_meta) {
            continue;
        }
        if file_meta.is_dir() {
            match fs::read_dir(&path) {
                Ok(entries) => stack.push(entries),
                Err(e) => {
                    complete = false;
                    let detail = if e.kind() == io::ErrorKind::PermissionDenied {
                        "Permission denied. Restore folder access, then rescan.".to_owned()
                    } else {
                        e.to_string()
                    };
                    failure(&mut progress, format!("{}: {detail}", path.display()));
                }
            }
            continue;
        }
        if !file_meta.is_file() || !metadata::supported(&path) {
            continue;
        }
        progress.visited += 1;
        progress.current = path.clone();
        let relative = path
            .strip_prefix(&root.path)
            .map_err(|_| LibraryError::Invalid("File escaped its music folder".into()))?
            .to_owned();
        let mut signature = signature(&file_meta)?;
        let identity = os_identity(&path);
        let mut previous = by_path.get(&metadata::path_key(&relative)).copied();
        let mut moved = previous.is_some_and(|old| old.file.location.relative_path != relative);
        let relocated = if previous.is_none() {
            if let Some(identity) = identity.clone() {
                let candidates = storage
                    .media_files_with_identity(identity)?
                    .into_iter()
                    .filter(|file| !seen.contains(&file.id))
                    .filter(|file| {
                        roots.get(&file.location.root).is_some_and(|old_root| {
                            fs::read_dir(&old_root.path).is_ok()
                                && fs::symlink_metadata(
                                    old_root.path.join(&file.location.relative_path),
                                )
                                .is_err_and(|error| error.kind() == io::ErrorKind::NotFound)
                        })
                    })
                    .collect::<Vec<_>>();
                match <[_; 1]>::try_from(candidates) {
                    Ok([file]) => Some(IndexedFile {
                        file,
                        projected: false,
                        metadata_error: false,
                    }),
                    Err(_) => None,
                }
            } else {
                None
            }
        } else {
            None
        };
        if let Some(file) = relocated.as_ref() {
            previous = Some(file);
            moved = true;
        }
        if let Some(old) = previous {
            seen.insert(old.file.id);
            if !moved
                && old.projected
                && !old.metadata_error
                && old.file.presence == Presence::Present
                && signature.matches_recorded(&old.file.signature)
                && old.file.os_file_id == identity
            {
                progress.unchanged += 1;
                publish(&progress);
                continue;
            }
        }
        if cancel.is_cancelled() {
            return Err(LibraryError::Cancelled);
        }
        signature.content_digest = Some(sampled_digest(&path, &signature)?);
        let (metadata, error) = match metadata::read_with_cancel(&path, cancel) {
            Ok(meta) => (meta, None),
            Err(e) => {
                let reason = format!("Can't read metadata: {e}");
                failure(&mut progress, format!("{}: {reason}", path.display()));
                let tags = match previous {
                    Some(old) => storage.media_file_tags(old.file.id)?,
                    None => Vec::new(),
                };
                (
                    Metadata {
                        tags,
                        duration_ms: 0,
                        format: "Unreadable".into(),
                    },
                    Some(reason),
                )
            }
        };
        if cancel.is_cancelled() {
            return Err(LibraryError::Cancelled);
        }
        let after = signature_for_path(&path)?;
        if !signature.matches_recorded(&after) {
            complete = false;
            failure(
                &mut progress,
                format!("{} changed during indexing", path.display()),
            );
            continue;
        }
        let projection = metadata.projection(root.id, &relative, error);
        if moved {
            let old = previous.unwrap();
            if let Some(old_key) = storage.local_group_key(old.file.id)? {
                // Reuse an album only when its old folder has actually disappeared.
                let parent = roots
                    .get(&old.file.location.root)
                    .map(|root| root.path.join(&old.file.location.relative_path))
                    .as_deref()
                    .and_then(Path::parent)
                    .map(Path::to_owned);
                if parent.is_some_and(|p| {
                    fs::metadata(p).is_err_and(|e| e.kind() == io::ErrorKind::NotFound)
                }) {
                    storage.move_local_group(old_key, projection.group_key.clone())?;
                }
            }
            storage.move_media_file(
                old.file.id,
                FileLocation {
                    root: root.id,
                    relative_path: relative.clone(),
                },
            )?;
        }
        let id = storage.record_media_file(MediaFileRecord {
            location: FileLocation {
                root: root.id,
                relative_path: relative,
            },
            os_file_id: identity,
            signature,
            tags: metadata.tags,
        })?;
        storage.save_song_metadata(id, projection)?;
        seen.insert(id);
        progress.indexed += 1;
        publish(&progress);
    }
    if cancel.is_cancelled() {
        progress.cancelled = true;
        publish(&progress);
        return Err(LibraryError::Cancelled);
    }
    // A drive disappearing during traversal must not turn all unseen entries into missing files.
    if fs::read_dir(&root.path).is_err() || os_identity(&root.path) != initial_id {
        storage.set_root_availability(root.id, RootAvailability::Offline)?;
        complete = false;
    }
    if complete {
        storage.reconcile_missing_files(
            known
                .iter()
                .filter(|f| !seen.contains(&f.file.id))
                .map(|f| f.file.id)
                .collect(),
        )?;
    }
    progress.finished = complete;
    publish(&progress);
    Ok(progress)
}

fn failure(progress: &mut ScanProgress, message: String) {
    progress.errors += 1;
    if progress.failures.len() < 32 {
        progress.failures.push(message);
    }
}
fn availability(error: &io::Error) -> RootAvailability {
    if error.kind() == io::ErrorKind::PermissionDenied {
        RootAvailability::PermissionDenied
    } else {
        RootAvailability::Offline
    }
}
fn is_link(meta: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        meta.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    {
        meta.file_type().is_symlink()
    }
}
pub fn os_identity(path: &Path) -> Option<OsFileId> {
    use file_id::FileId;
    let (volume, id) = match file_id::get_file_id(path).ok()? {
        FileId::Inode {
            device_id,
            inode_number,
        } => (format!("unix:{device_id}"), u128::from(inode_number)),
        FileId::LowRes {
            volume_serial_number,
            file_index,
        } => (
            format!("win:{volume_serial_number}"),
            u128::from(file_index),
        ),
        FileId::HighRes {
            volume_serial_number,
            file_id,
        } => (format!("win:{volume_serial_number}"), file_id),
    };
    Some(OsFileId {
        volume: VolumeId(volume),
        file_id: id,
    })
}
pub fn signature(meta: &fs::Metadata) -> io::Result<FileSignature> {
    let modified = meta.modified()?;
    let nanos = match modified.duration_since(UNIX_EPOCH) {
        Ok(d) => d.as_nanos() as i128,
        Err(e) => -(e.duration().as_nanos() as i128),
    };
    Ok(FileSignature {
        size_bytes: meta.len(),
        modified_unix_nanos: nanos,
        content_digest: None,
    })
}
pub fn signature_for_path(path: &Path) -> io::Result<FileSignature> {
    signature(&fs::metadata(path)?)
}

/// A bounded move hint, never used to merge ambiguous files automatically.
pub fn sampled_digest(path: &Path, signature: &FileSignature) -> io::Result<ContentDigest> {
    let mut file = File::open(path)?;
    let mut hash = Sha256::new();
    hash.update(signature.size_bytes.to_le_bytes());
    let mut bytes = [0; 65536];
    let n = signature.size_bytes.min(bytes.len() as u64) as usize;
    file.read_exact(&mut bytes[..n])?;
    hash.update(&bytes[..n]);
    if signature.size_bytes > 65536 {
        file.seek(SeekFrom::End(-65536))?;
        file.read_exact(&mut bytes)?;
        hash.update(bytes);
    }
    Ok(ContentDigest(hash.finalize().into()))
}

pub fn relocate_folder(
    storage: &Storage,
    root: LibraryRootId,
    path: &Path,
) -> Result<(), LibraryError> {
    let path = fs::canonicalize(path)?;
    let roots = storage.library_roots()?;
    if !path.is_dir() {
        return Err(LibraryError::Invalid("Choose a music folder".into()));
    }
    if roots
        .iter()
        .any(|r| r.id != root && (path.starts_with(&r.path) || r.path.starts_with(&path)))
    {
        return Err(LibraryError::Invalid(
            "This folder overlaps an existing music folder".into(),
        ));
    }
    let files = storage.indexed_files(root)?;
    if !files.is_empty()
        && !files.iter().any(|entry| {
            let candidate = path.join(&entry.file.location.relative_path);
            signature_for_path(&candidate).is_ok_and(|sig| {
                sig.size_bytes == entry.file.signature.size_bytes
                    && entry
                        .file
                        .signature
                        .content_digest
                        .is_some_and(|digest| sampled_digest(&candidate, &sig).ok() == Some(digest))
            })
        })
    {
        return Err(LibraryError::Invalid(
            "No matching indexed files found in this folder".into(),
        ));
    }
    storage.relocate_library_root(root, &path, os_identity(&path).map(|id| id.volume))?;
    Ok(())
}

pub fn recover_file(storage: &Storage, file: MediaFileId, path: &Path) -> Result<(), LibraryError> {
    let old = storage
        .media_file(file)?
        .ok_or_else(|| LibraryError::Invalid("File is no longer indexed".into()))?;
    let path = fs::canonicalize(path)?;
    let sig = signature_for_path(&path)?;
    if old.signature.size_bytes != sig.size_bytes
        || old.signature.content_digest.is_none()
        || Some(sampled_digest(&path, &sig)?) != old.signature.content_digest
    {
        return Err(LibraryError::Invalid(
            "The selected file does not match the missing file".into(),
        ));
    }
    let roots = storage.library_roots()?;
    let (root, relative) = roots
        .iter()
        .find_map(|root| {
            path.strip_prefix(&root.path)
                .ok()
                .map(|relative| (root.id, relative.to_owned()))
        })
        .ok_or_else(|| LibraryError::Invalid("Add the containing music folder first".into()))?;
    storage.recover_media_location(
        file,
        FileLocation {
            root,
            relative_path: relative,
        },
    )?;
    Ok(())
}
