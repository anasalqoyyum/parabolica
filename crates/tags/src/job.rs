//! Running a reviewed album write and recovering an interrupted one.
//!
//! Each target is recorded as `Replacing` (with its backup and the expected content) before the
//! rename and with its final outcome after it. [`recover`] resolves a `Replacing` record left by a
//! crash by comparing the file with both signatures. Multi-file writes are not atomic as a group.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use parabolica_domain::control::CancelToken;
use parabolica_domain::files::FileSignature;
use parabolica_domain::ids::{ArtworkId, JobId, MediaFileId};
use parabolica_domain::tags::{ArtworkDestination, PictureType, TagPatch};

use crate::artwork::{StagedImage, sidecar_name};
use crate::write::{self, FileEdit, Hooks, Preparation, WriteError};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetPlan {
    pub patch: TagPatch,
    pub path: PathBuf,
}

/// One sidecar image beside the album's files. `replace` lists the existing files the user chose to
/// replace at review.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SidecarPlan {
    pub directory: PathBuf,
    pub file_name: String,
    pub picture_type: PictureType,
    pub artwork: ArtworkId,
    pub replace: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TargetState {
    Pending,
    /// The verified file and backup exist and the rename is about to happen.
    Replacing {
        backup: PathBuf,
        expected: FileSignature,
    },
    Written {
        signature: FileSignature,
        backup: PathBuf,
    },
    /// The edit matched the file's existing tags and pictures.
    Unchanged,
    /// The file changed after review. Nothing was written; it needs a new review.
    Changed,
    Deferred {
        reason: String,
    },
    Failed {
        reason: String,
    },
    /// The backup was copied back after a write.
    Restored {
        signature: FileSignature,
    },
}

impl TargetState {
    pub fn succeeded(&self) -> bool {
        matches!(self, Self::Written { .. } | Self::Unchanged)
    }

    /// A retry reuses the reviewed patch; a changed file needs a new review instead.
    pub fn retryable(&self) -> bool {
        matches!(
            self,
            Self::Pending | Self::Deferred { .. } | Self::Failed { .. }
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SidecarState {
    Pending,
    Written,
    Changed,
    Failed { reason: String },
}

impl SidecarState {
    pub fn retryable(&self) -> bool {
        matches!(self, Self::Pending | Self::Failed { .. })
    }
}

/// Durable record of a running job. An error stops the job; the current file is not replaced.
pub trait Ledger {
    fn target(&mut self, file: MediaFileId, state: &TargetState) -> Result<(), String>;
    fn sidecar(&mut self, index: usize, state: &SidecarState) -> Result<(), String>;
}

#[derive(Debug, Clone, Copy)]
pub struct Job<'a> {
    pub id: JobId,
    pub targets: &'a [TargetPlan],
    pub sidecars: &'a [SidecarPlan],
    pub images: &'a HashMap<ArtworkId, StagedImage>,
    /// Backups for this job go into `backup_dir/<job id>/`.
    pub backup_dir: &'a Path,
}

pub fn temp_path(target: &Path, job: JobId) -> PathBuf {
    let name = target
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    target.with_file_name(format!(".{name}.{job}.parabolica-tmp"))
}

pub fn job_backup_dir(backup_dir: &Path, job: JobId) -> PathBuf {
    backup_dir.join(job.to_string())
}

fn backup_path(backup_dir: &Path, job: JobId, file: MediaFileId, target: &Path) -> PathBuf {
    let extension = target
        .extension()
        .map(|e| e.to_string_lossy().into_owned())
        .unwrap_or_default();
    job_backup_dir(backup_dir, job).join(format!("{file}.{extension}"))
}

/// Sidecar writes for the targets' "beside files" artwork, one set per folder and picture type.
/// `existing` returns the sidecar names reviewed in a folder for a picture type.
pub fn plan_sidecars(
    targets: &[TargetPlan],
    images: &HashMap<ArtworkId, StagedImage>,
    existing: impl Fn(&Path, PictureType) -> Vec<String>,
) -> Vec<SidecarPlan> {
    let mut groups: Vec<(PathBuf, PictureType, Vec<ArtworkId>)> = Vec::new();
    for target in targets {
        let Some(directory) = target.path.parent() else {
            continue;
        };
        for change in &target.patch.artwork {
            if change.destination == ArtworkDestination::Embed
                || change.picture_type == PictureType::Other
            {
                continue;
            }
            match groups
                .iter_mut()
                .find(|(dir, kind, _)| dir == directory && *kind == change.picture_type)
            {
                Some((_, _, ids)) if !ids.contains(&change.artwork) => ids.push(change.artwork),
                Some(_) => {}
                None => groups.push((
                    directory.to_owned(),
                    change.picture_type,
                    vec![change.artwork],
                )),
            }
        }
    }
    let mut plans = Vec::new();
    for (directory, kind, ids) in groups {
        let replace = existing(&directory, kind);
        for (index, artwork) in ids.iter().enumerate() {
            let Some(image) = images.get(artwork) else {
                continue;
            };
            if let Some(file_name) = sidecar_name(kind, index, ids.len(), image.kind()) {
                plans.push(SidecarPlan {
                    directory: directory.clone(),
                    file_name,
                    picture_type: kind,
                    artwork: *artwork,
                    replace: if index == 0 {
                        replace.clone()
                    } else {
                        Vec::new()
                    },
                });
            }
        }
    }
    plans
}

/// Write each target, then each sidecar. Returns `Ok(false)` when cancelled between files; the
/// remaining items stay `Pending`.
pub fn run(
    job: Job<'_>,
    ledger: &mut dyn Ledger,
    in_use: &dyn Fn(&Path) -> bool,
    cancel: &CancelToken,
    hooks: &dyn Hooks,
) -> Result<bool, String> {
    for target in job.targets {
        if cancel.is_cancelled() {
            return Ok(false);
        }
        let state = write_target(job, target, ledger, in_use, hooks)?;
        ledger.target(target.patch.target, &state)?;
    }
    let sidecar_backups = job_backup_dir(job.backup_dir, job.id).join("sidecars");
    for (index, sidecar) in job.sidecars.iter().enumerate() {
        if cancel.is_cancelled() {
            return Ok(false);
        }
        let state = match job.images.get(&sidecar.artwork) {
            None => SidecarState::Failed {
                reason: "Staged image is missing".into(),
            },
            Some(image) => match write::write_sidecar(
                &sidecar.directory,
                &sidecar.file_name,
                sidecar.picture_type,
                image,
                &sidecar.replace,
                &sidecar_backups.join(index.to_string()),
                &job.id.to_string(),
            ) {
                Ok(()) => SidecarState::Written,
                Err(WriteError::Changed) => SidecarState::Changed,
                Err(WriteError::InUse(reason) | WriteError::Failed(reason)) => {
                    SidecarState::Failed { reason }
                }
            },
        };
        ledger.sidecar(index, &state)?;
    }
    Ok(true)
}

fn write_target(
    job: Job<'_>,
    target: &TargetPlan,
    ledger: &mut dyn Ledger,
    in_use: &dyn Fn(&Path) -> bool,
    hooks: &dyn Hooks,
) -> Result<TargetState, String> {
    if in_use(&target.path) {
        return Ok(TargetState::Deferred {
            reason: "Playing in Parabolica".into(),
        });
    }
    let mut pictures = Vec::new();
    for change in &target.patch.artwork {
        if change.destination == ArtworkDestination::BesideFiles {
            continue;
        }
        let Some(image) = job.images.get(&change.artwork) else {
            return Ok(TargetState::Failed {
                reason: "Staged image is missing".into(),
            });
        };
        pictures.push((change.picture_type, image));
    }
    let backup = backup_path(job.backup_dir, job.id, target.patch.target, &target.path);
    let prepared = match write::prepare(
        &target.path,
        &target.patch.reviewed,
        FileEdit {
            fields: &target.patch.fields,
            pictures: &pictures,
        },
        &temp_path(&target.path, job.id),
        &backup,
        hooks,
    ) {
        Ok(Preparation::Ready(prepared)) => prepared,
        Ok(Preparation::Unchanged) => return Ok(TargetState::Unchanged),
        Err(error) => return Ok(error_state(error)),
    };
    let expected = match prepared.expected() {
        Ok(expected) => expected,
        Err(error) => {
            prepared.abandon();
            return Ok(TargetState::Failed {
                reason: error.to_string(),
            });
        }
    };
    if let Err(error) = ledger.target(
        target.patch.target,
        &TargetState::Replacing {
            backup: backup.clone(),
            expected,
        },
    ) {
        prepared.abandon();
        return Err(error);
    }
    Ok(match prepared.commit(hooks) {
        Ok(signature) => TargetState::Written { signature, backup },
        Err(error) => error_state(error),
    })
}

fn error_state(error: WriteError) -> TargetState {
    match error {
        WriteError::Changed => TargetState::Changed,
        WriteError::InUse(reason) => TargetState::Deferred { reason },
        WriteError::Failed(reason) => TargetState::Failed { reason },
    }
}

/// Resolve a target left `Pending` or `Replacing` by an interrupted job, and remove its temporary
/// file.
pub fn recover(
    job: JobId,
    path: &Path,
    reviewed: &FileSignature,
    state: &TargetState,
) -> TargetState {
    let _ = fs::remove_file(temp_path(path, job));
    match state {
        TargetState::Pending => TargetState::Failed {
            reason: "Interrupted before writing; the file is unchanged".into(),
        },
        TargetState::Replacing { backup, expected } => match write::fingerprint(path) {
            Ok(current) if same_content(&current, expected) => TargetState::Written {
                signature: current,
                backup: backup.clone(),
            },
            Ok(current) if same_content(&current, reviewed) => {
                let _ = fs::remove_file(backup);
                TargetState::Failed {
                    reason: "Interrupted before replacement; the file is unchanged".into(),
                }
            }
            Ok(_) => TargetState::Changed,
            Err(error) => TargetState::Failed {
                reason: write::failure(error, "File unavailable").to_string(),
            },
        },
        other => other.clone(),
    }
}

/// Remove a sidecar's temporary file left by an interrupted job.
pub fn recover_sidecar(job: JobId, sidecar: &SidecarPlan, state: &SidecarState) -> SidecarState {
    let _ = fs::remove_file(
        sidecar
            .directory
            .join(format!(".{}.{job}.parabolica-tmp", sidecar.file_name)),
    );
    match state {
        SidecarState::Pending => SidecarState::Failed {
            reason: "Interrupted before writing".into(),
        },
        other => other.clone(),
    }
}

fn same_content(a: &FileSignature, b: &FileSignature) -> bool {
    a.size_bytes == b.size_bytes
        && a.content_digest.is_some()
        && a.content_digest == b.content_digest
}
