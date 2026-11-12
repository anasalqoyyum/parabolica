//! Reviewed album tag writes: the patch for each file, per-file and per-sidecar outcomes, and
//! backups. The job's state lives in `jobs`; see `migrations/0012_01_tag_writes.sql`.

use std::path::PathBuf;

use parabolica_domain::files::FileSignature;
use parabolica_domain::ids::{ArtworkId, JobId, MediaFileId, ReleaseId};
use parabolica_domain::jobs::{JobState, Progress};
use parabolica_domain::tags::{
    ArtworkChange, ArtworkDestination, FieldChange, FieldEdit, PictureType, TagField, TagPatch,
};
use rusqlite::{OptionalExtension, Row, Transaction, params};

use crate::codec::{
    blob16, now_ms, optional_blob16, path_to_sql, relative_path_from_sql, signature_from_sql,
    signature_to_sql, sql_to_u64, u64_to_sql,
};
use crate::jobs::{state_from_sql, state_to_sql};
use crate::{Storage, StorageError};

/// A member of a local album, with its absolute path when its root is online and it is present.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AlbumEditFile {
    pub file: MediaFileId,
    pub path: Option<PathBuf>,
    pub disc: u32,
    pub track: u32,
    pub title: String,
    pub format: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TagWriteTarget {
    pub patch: TagPatch,
    pub path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TagWriteSidecar {
    pub directory: PathBuf,
    pub file_name: String,
    pub picture_type: PictureType,
    pub artwork: ArtworkId,
    pub replace: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TagTargetState {
    Pending,
    Replacing {
        backup: PathBuf,
        expected: FileSignature,
    },
    Written {
        signature: FileSignature,
        backup: PathBuf,
    },
    Unchanged,
    Changed,
    Deferred {
        reason: String,
    },
    Failed {
        reason: String,
    },
    Restored {
        signature: FileSignature,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TagSidecarState {
    Pending,
    Written,
    Changed,
    Failed { reason: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewTagWrite {
    pub album: ReleaseId,
    pub retry_of: Option<JobId>,
    pub targets: Vec<TagWriteTarget>,
    pub images: Vec<(ArtworkId, PathBuf)>,
    pub sidecars: Vec<TagWriteSidecar>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TagWriteRecord {
    pub id: JobId,
    pub album: ReleaseId,
    pub retry_of: Option<JobId>,
    pub state: JobState,
    pub created_at_ms: i64,
    pub backups_discarded: bool,
    pub targets: Vec<(TagWriteTarget, TagTargetState)>,
    pub images: Vec<(ArtworkId, PathBuf)>,
    pub sidecars: Vec<(TagWriteSidecar, TagSidecarState)>,
}

const HISTORY_LIMIT: i64 = 20;

impl Storage {
    /// Album members in disc and track order.
    pub fn album_edit_files(&self, album: ReleaseId) -> Result<Vec<AlbumEditFile>, StorageError> {
        self.read(move |tx| {
            let mut statement = tx.prepare(
                "SELECT s.file_id, s.disc, s.track, s.title, s.format, r.path, f.relative_path,
                        f.presence = 'present' AND r.availability = 'online'
                 FROM local_songs s
                 JOIN media_files f ON f.id = s.file_id
                 JOIN library_roots r ON r.id = f.root_id
                 WHERE s.album_id = ?1
                 ORDER BY s.disc, s.track, s.title, f.relative_path",
            )?;
            let mut rows = statement.query([album.to_bytes()])?;
            let mut files = Vec::new();
            while let Some(row) = rows.next()? {
                let root: String = row.get(5)?;
                let relative: String = row.get(6)?;
                let available: bool = row.get(7)?;
                files.push(AlbumEditFile {
                    file: MediaFileId::from_bytes(blob16(row, 0, "local_songs.file_id")?),
                    path: available
                        .then(|| PathBuf::from(root).join(relative_path_from_sql(&relative))),
                    disc: u32::try_from(row.get::<_, i64>(1)?).unwrap_or(0),
                    track: u32::try_from(row.get::<_, i64>(2)?).unwrap_or(0),
                    title: row.get(3)?,
                    format: row.get(4)?,
                });
            }
            Ok(files)
        })
    }

    /// The album a file currently belongs to. Writing album fields can regroup files into a new
    /// album, so the editor follows its files.
    pub fn album_of_file(&self, file: MediaFileId) -> Result<Option<ReleaseId>, StorageError> {
        self.read(move |tx| {
            let album: Option<Vec<u8>> = tx
                .query_row(
                    "SELECT album_id FROM local_songs WHERE file_id = ?1",
                    [file.to_bytes()],
                    |row| row.get(0),
                )
                .optional()?;
            album
                .map(|bytes| {
                    bytes.try_into().map(ReleaseId::from_bytes).map_err(|_| {
                        StorageError::InvalidStoredValue {
                            column: "local_songs.album_id",
                        }
                    })
                })
                .transpose()
        })
    }

    /// Record a reviewed write as a queued job. Every patch must carry a full-content digest, and
    /// a file appears at most once.
    pub fn create_tag_write(&self, write: NewTagWrite) -> Result<JobId, StorageError> {
        if write.targets.is_empty() && write.sidecars.is_empty() {
            return Err(StorageError::invalid("tag write", "has nothing to write"));
        }
        let mut seen = std::collections::BTreeSet::new();
        for target in &write.targets {
            if !seen.insert(target.patch.target) {
                return Err(StorageError::invalid("tag write", "patches a file twice"));
            }
            if target.patch.reviewed.content_digest.is_none() {
                return Err(StorageError::invalid(
                    "tag write",
                    "has a reviewed signature without a content digest",
                ));
            }
        }
        self.write(move |tx| {
            let id = JobId::generate();
            let now = now_ms();
            let total = u64_to_sql(
                (write.targets.len() + write.sidecars.len()) as u64,
                "tag write size",
            )?;
            tx.execute(
                "INSERT INTO jobs (id, kind, state, progress_done, progress_total, created_at_ms, updated_at_ms)
                 VALUES (?1, 'tag-write', 'queued', 0, ?2, ?3, ?3)",
                params![id.to_bytes(), total, now],
            )?;
            tx.execute(
                "INSERT INTO tag_writes (job_id, album_id, retry_of) VALUES (?1, ?2, ?3)",
                params![
                    id.to_bytes(),
                    write.album.to_bytes(),
                    write.retry_of.map(JobId::to_bytes)
                ],
            )?;
            for (artwork, path) in &write.images {
                tx.execute(
                    "INSERT INTO tag_write_images (job_id, artwork_id, staged_path) VALUES (?1, ?2, ?3)",
                    params![id.to_bytes(), artwork.to_bytes(), path_to_sql(path)?],
                )?;
            }
            for (ordinal, target) in write.targets.iter().enumerate() {
                insert_target(tx, id, ordinal, target)?;
            }
            for (ordinal, sidecar) in write.sidecars.iter().enumerate() {
                tx.execute(
                    "INSERT INTO tag_write_sidecars (job_id, ordinal, directory, file_name,
                         picture_type, artwork_id, replace_names, state)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'pending')",
                    params![
                        id.to_bytes(),
                        ordinal as i64,
                        path_to_sql(&sidecar.directory)?,
                        sidecar.file_name,
                        picture_to_sql(sidecar.picture_type),
                        sidecar.artwork.to_bytes(),
                        sidecar.replace.join("/"),
                    ],
                )?;
            }
            Ok(id)
        })
    }

    /// Queued → running.
    pub fn start_tag_write(&self, job: JobId) -> Result<(), StorageError> {
        self.write(move |tx| {
            let (state, done, total) = job_state(tx, job)?;
            let progress = Progress { done, total };
            set_job_state(tx, job, &state, JobState::Running(progress))
        })
    }

    /// Record a target's state while the job runs.
    pub fn record_tag_target(
        &self,
        job: JobId,
        file: MediaFileId,
        state: TagTargetState,
    ) -> Result<(), StorageError> {
        self.write(move |tx| {
            require_running(tx, job)?;
            update_target(tx, job, file, &state)?;
            update_progress(tx, job)
        })
    }

    pub fn record_tag_sidecar(
        &self,
        job: JobId,
        ordinal: usize,
        state: TagSidecarState,
    ) -> Result<(), StorageError> {
        self.write(move |tx| {
            require_running(tx, job)?;
            let (state, detail) = match &state {
                TagSidecarState::Pending => ("pending", None),
                TagSidecarState::Written => ("written", None),
                TagSidecarState::Changed => ("changed", None),
                TagSidecarState::Failed { reason } => ("failed", Some(reason.as_str())),
            };
            let changed = tx.execute(
                "UPDATE tag_write_sidecars SET state = ?3, detail = ?4 WHERE job_id = ?1 AND ordinal = ?2",
                params![job.to_bytes(), ordinal as i64, state, detail],
            )?;
            if changed == 0 {
                return Err(StorageError::NotFound(format!("sidecar {ordinal} of {job}")));
            }
            update_progress(tx, job)
        })
    }

    /// End a running job: Completed when every file and sidecar succeeded, Incomplete when some
    /// did, Failed when none did, or Cancelled. A queued job can only be cancelled.
    pub fn finish_tag_write(&self, job: JobId, cancelled: bool) -> Result<JobState, StorageError> {
        self.write(move |tx| {
            let (current, _, _) = job_state(tx, job)?;
            let next = if cancelled || current == JobState::Queued {
                JobState::Cancelled
            } else {
                let count = |sql: &str| -> Result<i64, StorageError> {
                    Ok(tx.query_row(sql, [job.to_bytes()], |row| row.get(0))?)
                };
                let total = count("SELECT count(*) FROM tag_write_targets WHERE job_id = ?1")?
                    + count("SELECT count(*) FROM tag_write_sidecars WHERE job_id = ?1")?;
                let succeeded = count(
                    "SELECT count(*) FROM tag_write_targets
                     WHERE job_id = ?1 AND state IN ('written', 'unchanged', 'restored')",
                )? + count(
                    "SELECT count(*) FROM tag_write_sidecars WHERE job_id = ?1 AND state = 'written'",
                )?;
                if succeeded == total {
                    JobState::Completed
                } else if succeeded > 0 {
                    JobState::Incomplete
                } else {
                    JobState::Failed {
                        reason: "Nothing was written".into(),
                    }
                }
            };
            set_job_state(tx, job, &current, next.clone())?;
            Ok(next)
        })
    }

    /// Record that a written file's backup was copied back, after the job ended.
    pub fn record_tag_restore(
        &self,
        job: JobId,
        file: MediaFileId,
        signature: FileSignature,
    ) -> Result<(), StorageError> {
        self.write(move |tx| {
            let state: Option<String> = tx
                .query_row(
                    "SELECT state FROM tag_write_targets WHERE job_id = ?1 AND file_id = ?2",
                    params![job.to_bytes(), file.to_bytes()],
                    |row| row.get(0),
                )
                .optional()?;
            match state.as_deref() {
                Some("written") => {
                    update_target(tx, job, file, &TagTargetState::Restored { signature })
                }
                Some(_) => Err(StorageError::Conflict(format!(
                    "{file} in {job} has no written content to restore"
                ))),
                None => Err(StorageError::NotFound(format!("{file} in {job}"))),
            }
        })
    }

    /// Mark a finished job's backups as deleted, so restore is no longer offered.
    pub fn discard_tag_backups(&self, job: JobId) -> Result<(), StorageError> {
        self.write(move |tx| {
            let changed = tx.execute(
                "UPDATE tag_writes SET backups_discarded = 1 WHERE job_id = ?1
                 AND job_id IN (SELECT id FROM jobs WHERE state NOT IN ('queued', 'running'))",
                [job.to_bytes()],
            )?;
            if changed == 0 {
                return Err(StorageError::Conflict(format!(
                    "{job} is unfinished or unknown"
                )));
            }
            Ok(())
        })
    }

    pub fn tag_write(&self, job: JobId) -> Result<Option<TagWriteRecord>, StorageError> {
        self.read(move |tx| load_record(tx, job))
    }

    /// The most recent writes made for the album or to any of its current files, newest first.
    /// A write can regroup files into a new album; its history moves with them.
    pub fn tag_writes(&self, album: ReleaseId) -> Result<Vec<TagWriteRecord>, StorageError> {
        self.read(move |tx| {
            let ids = job_ids(
                tx,
                "SELECT t.job_id FROM tag_writes t JOIN jobs j ON j.id = t.job_id
                 WHERE t.album_id = ?1 OR t.job_id IN (
                     SELECT w.job_id FROM tag_write_targets w
                     JOIN local_songs s ON s.file_id = w.file_id WHERE s.album_id = ?1)
                 ORDER BY j.created_at_ms DESC, j.id DESC LIMIT ?2",
                params![album.to_bytes(), HISTORY_LIMIT],
            )?;
            ids.into_iter()
                .filter_map(|id| load_record(tx, id).transpose())
                .collect()
        })
    }

    /// Queued and running writes, which after a restart were interrupted.
    pub fn unfinished_tag_writes(&self) -> Result<Vec<TagWriteRecord>, StorageError> {
        self.read(|tx| {
            let ids = job_ids(
                tx,
                "SELECT t.job_id FROM tag_writes t JOIN jobs j ON j.id = t.job_id
                 WHERE j.state IN ('queued', 'running') ORDER BY j.created_at_ms, j.id",
                params![],
            )?;
            ids.into_iter()
                .filter_map(|id| load_record(tx, id).transpose())
                .collect()
        })
    }
}

fn insert_target(
    tx: &Transaction<'_>,
    job: JobId,
    ordinal: usize,
    target: &TagWriteTarget,
) -> Result<(), StorageError> {
    let patch = &target.patch;
    let reviewed = signature_to_sql(&patch.reviewed)?;
    let file = patch.target.to_bytes();
    tx.execute(
        "INSERT INTO tag_write_targets (job_id, file_id, ordinal, path, reviewed_size_bytes,
             reviewed_modified_unix_nanos, reviewed_digest, state)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'pending')",
        params![
            job.to_bytes(),
            file,
            ordinal as i64,
            path_to_sql(&target.path)?,
            reviewed.size_bytes,
            reviewed.modified_unix_nanos,
            reviewed.digest,
        ],
    )?;
    for (field_ordinal, change) in patch.fields.iter().enumerate() {
        let (edit, values): (&str, &[String]) = match &change.edit {
            FieldEdit::Replace(values) => ("replace", values),
            FieldEdit::FillIfEmpty(values) => ("fill", values),
            FieldEdit::Remove => ("remove", &[]),
        };
        tx.execute(
            "INSERT INTO tag_write_fields (job_id, file_id, ordinal, field, edit)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                job.to_bytes(),
                file,
                field_ordinal as i64,
                field_to_sql(&change.field),
                edit
            ],
        )?;
        for (value_ordinal, value) in values.iter().enumerate() {
            tx.execute(
                "INSERT INTO tag_write_values (job_id, file_id, field_ordinal, ordinal, value)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    job.to_bytes(),
                    file,
                    field_ordinal as i64,
                    value_ordinal as i64,
                    value
                ],
            )?;
        }
    }
    for (artwork_ordinal, change) in patch.artwork.iter().enumerate() {
        tx.execute(
            "INSERT INTO tag_write_artwork (job_id, file_id, ordinal, artwork_id, picture_type, destination)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                job.to_bytes(),
                file,
                artwork_ordinal as i64,
                change.artwork.to_bytes(),
                picture_to_sql(change.picture_type),
                match change.destination {
                    ArtworkDestination::Embed => "embed",
                    ArtworkDestination::BesideFiles => "beside",
                    ArtworkDestination::Both => "both",
                },
            ],
        )?;
    }
    Ok(())
}

fn update_target(
    tx: &Transaction<'_>,
    job: JobId,
    file: MediaFileId,
    state: &TagTargetState,
) -> Result<(), StorageError> {
    let (name, detail, backup, result): (_, Option<&str>, Option<&PathBuf>, _) = match state {
        TagTargetState::Pending => ("pending", None, None, None),
        TagTargetState::Replacing { backup, expected } => {
            ("replacing", None, Some(backup), Some(expected))
        }
        TagTargetState::Written { signature, backup } => {
            ("written", None, Some(backup), Some(signature))
        }
        TagTargetState::Unchanged => ("unchanged", None, None, None),
        TagTargetState::Changed => ("changed", None, None, None),
        TagTargetState::Deferred { reason } => ("deferred", Some(reason.as_str()), None, None),
        TagTargetState::Failed { reason } => ("failed", Some(reason.as_str()), None, None),
        TagTargetState::Restored { signature } => ("restored", None, None, Some(signature)),
    };
    let result = result.map(signature_to_sql).transpose()?;
    if result.as_ref().is_some_and(|r| r.digest.is_none()) {
        return Err(StorageError::invalid(
            "tag write result",
            "has no content digest",
        ));
    }
    let backup = backup.map(|path| path_to_sql(path)).transpose()?;
    let changed = tx.execute(
        "UPDATE tag_write_targets SET state = ?3, detail = ?4, backup_path = ?5,
             result_size_bytes = ?6, result_modified_unix_nanos = ?7, result_digest = ?8
         WHERE job_id = ?1 AND file_id = ?2",
        params![
            job.to_bytes(),
            file.to_bytes(),
            name,
            detail,
            backup,
            result.as_ref().map(|r| r.size_bytes),
            result.as_ref().map(|r| r.modified_unix_nanos),
            result.as_ref().and_then(|r| r.digest),
        ],
    )?;
    if changed == 0 {
        return Err(StorageError::NotFound(format!("{file} in {job}")));
    }
    Ok(())
}

fn update_progress(tx: &Transaction<'_>, job: JobId) -> Result<(), StorageError> {
    tx.execute(
        "UPDATE jobs SET updated_at_ms = ?2, progress_done =
             (SELECT count(*) FROM tag_write_targets WHERE job_id = ?1 AND state NOT IN ('pending', 'replacing'))
             + (SELECT count(*) FROM tag_write_sidecars WHERE job_id = ?1 AND state <> 'pending')
         WHERE id = ?1",
        params![job.to_bytes(), now_ms()],
    )?;
    Ok(())
}

fn job_state(
    tx: &Transaction<'_>,
    job: JobId,
) -> Result<(JobState, u64, Option<u64>), StorageError> {
    let row: Option<(String, Option<String>, i64, Option<i64>)> = tx
        .query_row(
            "SELECT j.state, j.failure_reason, j.progress_done, j.progress_total
             FROM jobs j JOIN tag_writes t ON t.job_id = j.id WHERE j.id = ?1",
            [job.to_bytes()],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .optional()?;
    let (state, reason, done, total) =
        row.ok_or_else(|| StorageError::NotFound(job.to_string()))?;
    let done = sql_to_u64(done, "jobs.progress_done")?;
    let total = total
        .map(|total| sql_to_u64(total, "jobs.progress_total"))
        .transpose()?;
    Ok((
        state_from_sql(&state, reason, Progress { done, total })?,
        done,
        total,
    ))
}

fn require_running(tx: &Transaction<'_>, job: JobId) -> Result<(), StorageError> {
    match job_state(tx, job)?.0 {
        JobState::Running(_) => Ok(()),
        _ => Err(StorageError::Conflict(format!("{job} is not running"))),
    }
}

fn set_job_state(
    tx: &Transaction<'_>,
    job: JobId,
    current: &JobState,
    next: JobState,
) -> Result<(), StorageError> {
    let next = current
        .transition(next)
        .map_err(|invalid| StorageError::JobTransition {
            from: state_to_sql(&invalid.from).0,
            to: state_to_sql(&invalid.to).0,
        })?;
    let (state, reason) = state_to_sql(&next);
    tx.execute(
        "UPDATE jobs SET state = ?2, failure_reason = ?3, updated_at_ms = ?4 WHERE id = ?1",
        params![job.to_bytes(), state, reason, now_ms()],
    )?;
    Ok(())
}

fn job_ids(
    tx: &Transaction<'_>,
    sql: &str,
    params: impl rusqlite::Params,
) -> Result<Vec<JobId>, StorageError> {
    let mut statement = tx.prepare(sql)?;
    let mut rows = statement.query(params)?;
    let mut ids = Vec::new();
    while let Some(row) = rows.next()? {
        ids.push(JobId::from_bytes(blob16(row, 0, "tag_writes.job_id")?));
    }
    Ok(ids)
}

fn load_record(tx: &Transaction<'_>, job: JobId) -> Result<Option<TagWriteRecord>, StorageError> {
    let mut statement = tx.prepare(
        "SELECT t.album_id, t.retry_of, t.backups_discarded, j.created_at_ms
         FROM tag_writes t JOIN jobs j ON j.id = t.job_id WHERE t.job_id = ?1",
    )?;
    let mut rows = statement.query([job.to_bytes()])?;
    let Some(row) = rows.next()? else {
        return Ok(None);
    };
    let album = ReleaseId::from_bytes(blob16(row, 0, "tag_writes.album_id")?);
    let retry_of = optional_blob16(row, 1, "tag_writes.retry_of")?.map(JobId::from_bytes);
    let backups_discarded: bool = row.get(2)?;
    let created_at_ms: i64 = row.get(3)?;
    let (state, _, _) = job_state(tx, job)?;

    let mut images = Vec::new();
    {
        let mut statement = tx.prepare(
            "SELECT artwork_id, staged_path FROM tag_write_images WHERE job_id = ?1 ORDER BY artwork_id",
        )?;
        let mut rows = statement.query([job.to_bytes()])?;
        while let Some(row) = rows.next()? {
            images.push((
                ArtworkId::from_bytes(blob16(row, 0, "tag_write_images.artwork_id")?),
                PathBuf::from(row.get::<_, String>(1)?),
            ));
        }
    }

    let mut targets = Vec::new();
    {
        let mut statement = tx.prepare(
            "SELECT file_id, path, reviewed_size_bytes, reviewed_modified_unix_nanos, reviewed_digest,
                    state, detail, backup_path, result_size_bytes, result_modified_unix_nanos,
                    result_digest
             FROM tag_write_targets WHERE job_id = ?1 ORDER BY ordinal",
        )?;
        let mut rows = statement.query([job.to_bytes()])?;
        while let Some(row) = rows.next()? {
            let file = MediaFileId::from_bytes(blob16(row, 0, "tag_write_targets.file_id")?);
            let reviewed = signature_from_sql(row.get(2)?, row.get(3)?, Some(row.get(4)?))?;
            let state = target_state_from_row(row)?;
            let (fields, artwork) = load_patch(tx, job, file)?;
            targets.push((
                TagWriteTarget {
                    patch: TagPatch {
                        target: file,
                        reviewed,
                        fields,
                        artwork,
                    },
                    path: PathBuf::from(row.get::<_, String>(1)?),
                },
                state,
            ));
        }
    }

    let mut sidecars = Vec::new();
    {
        let mut statement = tx.prepare(
            "SELECT directory, file_name, picture_type, artwork_id, replace_names, state, detail
             FROM tag_write_sidecars WHERE job_id = ?1 ORDER BY ordinal",
        )?;
        let mut rows = statement.query([job.to_bytes()])?;
        while let Some(row) = rows.next()? {
            let replace: String = row.get(4)?;
            let state = match (
                row.get::<_, String>(5)?.as_str(),
                row.get::<_, Option<String>>(6)?,
            ) {
                ("pending", _) => TagSidecarState::Pending,
                ("written", _) => TagSidecarState::Written,
                ("changed", _) => TagSidecarState::Changed,
                ("failed", Some(reason)) => TagSidecarState::Failed { reason },
                _ => {
                    return Err(StorageError::InvalidStoredValue {
                        column: "tag_write_sidecars.state",
                    });
                }
            };
            sidecars.push((
                TagWriteSidecar {
                    directory: PathBuf::from(row.get::<_, String>(0)?),
                    file_name: row.get(1)?,
                    picture_type: picture_from_sql(&row.get::<_, String>(2)?)?,
                    artwork: ArtworkId::from_bytes(blob16(
                        row,
                        3,
                        "tag_write_sidecars.artwork_id",
                    )?),
                    replace: if replace.is_empty() {
                        Vec::new()
                    } else {
                        replace.split('/').map(str::to_owned).collect()
                    },
                },
                state,
            ));
        }
    }

    Ok(Some(TagWriteRecord {
        id: job,
        album,
        retry_of,
        state,
        created_at_ms,
        backups_discarded,
        targets,
        images,
        sidecars,
    }))
}

fn target_state_from_row(row: &Row<'_>) -> Result<TagTargetState, StorageError> {
    let state: String = row.get(5)?;
    let detail: Option<String> = row.get(6)?;
    let backup = row.get::<_, Option<String>>(7)?.map(PathBuf::from);
    let result = match (
        row.get::<_, Option<i64>>(8)?,
        row.get::<_, Option<i64>>(9)?,
        row.get::<_, Option<Vec<u8>>>(10)?,
    ) {
        (Some(size), Some(modified), Some(digest)) => {
            Some(signature_from_sql(size, modified, Some(digest))?)
        }
        _ => None,
    };
    let invalid = || StorageError::InvalidStoredValue {
        column: "tag_write_targets.state",
    };
    Ok(match state.as_str() {
        "pending" => TagTargetState::Pending,
        "replacing" => TagTargetState::Replacing {
            backup: backup.ok_or_else(invalid)?,
            expected: result.ok_or_else(invalid)?,
        },
        "written" => TagTargetState::Written {
            signature: result.ok_or_else(invalid)?,
            backup: backup.ok_or_else(invalid)?,
        },
        "unchanged" => TagTargetState::Unchanged,
        "changed" => TagTargetState::Changed,
        "deferred" => TagTargetState::Deferred {
            reason: detail.ok_or_else(invalid)?,
        },
        "failed" => TagTargetState::Failed {
            reason: detail.ok_or_else(invalid)?,
        },
        "restored" => TagTargetState::Restored {
            signature: result.ok_or_else(invalid)?,
        },
        _ => return Err(invalid()),
    })
}

fn load_patch(
    tx: &Transaction<'_>,
    job: JobId,
    file: MediaFileId,
) -> Result<(Vec<FieldChange>, Vec<ArtworkChange>), StorageError> {
    let mut fields = Vec::new();
    let mut statement = tx.prepare(
        "SELECT ordinal, field, edit FROM tag_write_fields WHERE job_id = ?1 AND file_id = ?2
         ORDER BY ordinal",
    )?;
    let mut values = tx.prepare(
        "SELECT value FROM tag_write_values WHERE job_id = ?1 AND file_id = ?2 AND field_ordinal = ?3
         ORDER BY ordinal",
    )?;
    let mut rows = statement.query(params![job.to_bytes(), file.to_bytes()])?;
    while let Some(row) = rows.next()? {
        let ordinal: i64 = row.get(0)?;
        let field = field_from_sql(&row.get::<_, String>(1)?)?;
        let list = values
            .query_map(params![job.to_bytes(), file.to_bytes(), ordinal], |row| {
                row.get::<_, String>(0)
            })?
            .collect::<Result<Vec<_>, _>>()?;
        let edit = match row.get::<_, String>(2)?.as_str() {
            "replace" => FieldEdit::Replace(list),
            "fill" => FieldEdit::FillIfEmpty(list),
            "remove" => FieldEdit::Remove,
            _ => {
                return Err(StorageError::InvalidStoredValue {
                    column: "tag_write_fields.edit",
                });
            }
        };
        fields.push(FieldChange { field, edit });
    }
    let mut artwork = Vec::new();
    let mut statement = tx.prepare(
        "SELECT artwork_id, picture_type, destination FROM tag_write_artwork
         WHERE job_id = ?1 AND file_id = ?2 ORDER BY ordinal",
    )?;
    let mut rows = statement.query(params![job.to_bytes(), file.to_bytes()])?;
    while let Some(row) = rows.next()? {
        artwork.push(ArtworkChange {
            artwork: ArtworkId::from_bytes(blob16(row, 0, "tag_write_artwork.artwork_id")?),
            picture_type: picture_from_sql(&row.get::<_, String>(1)?)?,
            destination: match row.get::<_, String>(2)?.as_str() {
                "embed" => ArtworkDestination::Embed,
                "beside" => ArtworkDestination::BesideFiles,
                "both" => ArtworkDestination::Both,
                _ => {
                    return Err(StorageError::InvalidStoredValue {
                        column: "tag_write_artwork.destination",
                    });
                }
            },
        });
    }
    Ok((fields, artwork))
}

const FIELDS: [(TagField, &str); 18] = [
    (TagField::Title, "title"),
    (TagField::Artist, "artist"),
    (TagField::Album, "album"),
    (TagField::AlbumArtist, "album-artist"),
    (TagField::TrackNumber, "track-number"),
    (TagField::TrackTotal, "track-total"),
    (TagField::DiscNumber, "disc-number"),
    (TagField::DiscTotal, "disc-total"),
    (TagField::Date, "date"),
    (TagField::Genre, "genre"),
    (TagField::Style, "style"),
    (TagField::Label, "label"),
    (TagField::CatalogNumber, "catalog-number"),
    (TagField::Country, "country"),
    (TagField::ReplayGainTrackGain, "replaygain-track-gain"),
    (TagField::ReplayGainTrackPeak, "replaygain-track-peak"),
    (TagField::ReplayGainAlbumGain, "replaygain-album-gain"),
    (TagField::ReplayGainAlbumPeak, "replaygain-album-peak"),
];

fn field_to_sql(field: &TagField) -> String {
    match field {
        TagField::Custom(name) => format!("custom:{name}"),
        known => FIELDS
            .iter()
            .find(|(field, _)| field == known)
            .map(|(_, name)| (*name).to_owned())
            .unwrap_or_default(),
    }
}

fn field_from_sql(text: &str) -> Result<TagField, StorageError> {
    if let Some(name) = text.strip_prefix("custom:") {
        return Ok(TagField::Custom(name.to_owned()));
    }
    FIELDS
        .iter()
        .find(|(_, name)| *name == text)
        .map(|(field, _)| field.clone())
        .ok_or(StorageError::InvalidStoredValue {
            column: "tag_write_fields.field",
        })
}

fn picture_to_sql(kind: PictureType) -> &'static str {
    match kind {
        PictureType::FrontCover => "front",
        PictureType::BackCover => "back",
        PictureType::Booklet => "booklet",
        PictureType::Other => "other",
    }
}

fn picture_from_sql(text: &str) -> Result<PictureType, StorageError> {
    Ok(match text {
        "front" => PictureType::FrontCover,
        "back" => PictureType::BackCover,
        "booklet" => PictureType::Booklet,
        "other" => PictureType::Other,
        _ => {
            return Err(StorageError::InvalidStoredValue {
                column: "picture_type",
            });
        }
    })
}
