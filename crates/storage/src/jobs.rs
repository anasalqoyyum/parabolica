//! Durable background job state with resumable items.
//!
//! A job's state changes only through [`JobState::transition`], checked against the stored state
//! inside the write transaction, so concurrent workers cannot complete a cancelled job. Item
//! outcomes are accepted only while the job runs. After a restart, [`Storage::unfinished_jobs`]
//! lists queued and running jobs; their owner resumes the items that are still pending.

use parabolica_domain::files::FileSignature;
use parabolica_domain::ids::{JobId, MediaFileId};
use parabolica_domain::jobs::{JobKind, JobState, Progress};
use rusqlite::{Row, Transaction, params};

use crate::codec::{
    blob16, now_ms, optional_blob16, signature_from_sql, signature_to_sql, sql_to_u64, u64_to_sql,
};
use crate::{Storage, StorageError};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobRecord {
    pub id: JobId,
    pub kind: JobKind,
    pub state: JobState,
    /// Last reported progress; for a running job it equals the progress in `state`.
    pub progress: Progress,
}

/// One unit of work, such as a file to scan or write. `key` is unique within the job and chosen by
/// its owner (for example the file ID's text form).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewJobItem {
    pub key: String,
    pub file: Option<MediaFileId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemOutcome {
    Pending,
    Done,
    Skipped,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobItem {
    pub key: String,
    pub file: Option<MediaFileId>,
    pub outcome: ItemOutcome,
    /// A failure or skip reason to show the user.
    pub detail: Option<String>,
    /// The file signature the outcome was recorded against. A resumed job reruns an item whose
    /// file no longer matches it.
    pub signature: Option<FileSignature>,
}

/// A finished (or reset) item reported by a worker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemReport {
    pub key: String,
    pub outcome: ItemOutcome,
    pub detail: Option<String>,
    pub signature: Option<FileSignature>,
}

impl Storage {
    /// Create a queued job with its items, in order.
    pub fn create_job(&self, kind: JobKind, items: Vec<NewJobItem>) -> Result<JobId, StorageError> {
        if items.iter().any(|item| item.key.is_empty()) {
            return Err(StorageError::invalid("job item", "has an empty key"));
        }
        let total = (!items.is_empty())
            .then(|| u64_to_sql(items.len() as u64, "job size"))
            .transpose()?;
        self.write(move |tx| {
            let id = JobId::generate();
            let now = now_ms();
            tx.execute(
                "INSERT INTO jobs (id, kind, state, progress_done, progress_total, created_at_ms, updated_at_ms)
                 VALUES (?1, ?2, 'queued', 0, ?3, ?4, ?4)",
                params![id.to_bytes(), kind_to_sql(kind), total, now],
            )?;
            let mut insert = tx.prepare(
                "INSERT INTO job_items (job_id, item_key, ordinal, file_id, outcome, updated_at_ms)
                 VALUES (?1, ?2, ?3, ?4, 'pending', ?5)",
            )?;
            for (ordinal, item) in items.iter().enumerate() {
                insert.execute(params![
                    id.to_bytes(),
                    item.key,
                    ordinal as i64,
                    item.file.map(MediaFileId::to_bytes),
                    now,
                ])?;
            }
            Ok(id)
        })
    }

    /// Move a job to `to`. Fails with [`StorageError::JobTransition`] when the stored state does not
    /// allow it, and refuses `Completed` while any item is pending, skipped or failed.
    pub fn transition_job(&self, job: JobId, to: JobState) -> Result<(), StorageError> {
        self.write(move |tx| {
            let current =
                load_job(tx, job)?.ok_or_else(|| StorageError::NotFound(job.to_string()))?;
            let next = current.state.transition(to)?;
            if next == JobState::Completed {
                let unfinished: i64 = tx.query_row(
                    "SELECT count(*) FROM job_items WHERE job_id = ?1 AND outcome <> 'done'",
                    [job.to_bytes()],
                    |row| row.get(0),
                )?;
                if unfinished > 0 {
                    return Err(StorageError::invalid(
                        "job completion",
                        "items are pending, skipped or failed",
                    ));
                }
            }
            let progress = match &next {
                JobState::Running(progress) => *progress,
                _ => current.progress,
            };
            let (state, reason) = state_to_sql(&next);
            tx.execute(
                "UPDATE jobs SET state = ?2, failure_reason = ?3, progress_done = ?4,
                     progress_total = ?5, updated_at_ms = ?6
                 WHERE id = ?1",
                params![
                    job.to_bytes(),
                    state,
                    reason,
                    u64_to_sql(progress.done, "job progress")?,
                    progress
                        .total
                        .map(|total| u64_to_sql(total, "job size"))
                        .transpose()?,
                    now_ms(),
                ],
            )?;
            Ok(())
        })
    }

    /// Record item outcomes from a running job and update its progress to the number of finished
    /// items, in one transaction. Fails with [`StorageError::Conflict`] once the job has stopped
    /// running, so a late report from a cancelled worker changes nothing.
    pub fn record_job_items(
        &self,
        job: JobId,
        reports: Vec<ItemReport>,
    ) -> Result<(), StorageError> {
        let mut rows = Vec::with_capacity(reports.len());
        for report in reports {
            let signature = report
                .signature
                .as_ref()
                .map(signature_to_sql)
                .transpose()?;
            rows.push((report, signature));
        }
        self.write(move |tx| {
            let current =
                load_job(tx, job)?.ok_or_else(|| StorageError::NotFound(job.to_string()))?;
            if !matches!(current.state, JobState::Running(_)) {
                return Err(StorageError::Conflict(format!("{job} is not running")));
            }
            let now = now_ms();
            let mut update = tx.prepare(
                "UPDATE job_items SET outcome = ?3, detail = ?4, signature_size_bytes = ?5,
                     signature_modified_unix_nanos = ?6, signature_digest = ?7, updated_at_ms = ?8
                 WHERE job_id = ?1 AND item_key = ?2",
            )?;
            for (report, signature) in &rows {
                let changed = update.execute(params![
                    job.to_bytes(),
                    report.key,
                    outcome_to_sql(report.outcome),
                    report.detail,
                    signature.as_ref().map(|signature| signature.size_bytes),
                    signature
                        .as_ref()
                        .map(|signature| signature.modified_unix_nanos),
                    signature.as_ref().and_then(|signature| signature.digest),
                    now,
                ])?;
                if changed == 0 {
                    return Err(StorageError::NotFound(format!(
                        "item {} of {job}",
                        report.key
                    )));
                }
            }
            tx.execute(
                "UPDATE jobs SET updated_at_ms = ?2, progress_done = (
                     SELECT count(*) FROM job_items WHERE job_id = ?1 AND outcome <> 'pending')
                 WHERE id = ?1",
                params![job.to_bytes(), now],
            )?;
            Ok(())
        })
    }

    pub fn job(&self, job: JobId) -> Result<Option<JobRecord>, StorageError> {
        self.read(move |tx| load_job(tx, job))
    }

    pub fn job_items(&self, job: JobId) -> Result<Vec<JobItem>, StorageError> {
        self.read(move |tx| {
            let mut statement = tx.prepare(
                "SELECT item_key, file_id, outcome, detail, signature_size_bytes,
                        signature_modified_unix_nanos, signature_digest
                 FROM job_items WHERE job_id = ?1 ORDER BY ordinal",
            )?;
            let mut rows = statement.query([job.to_bytes()])?;
            let mut items = Vec::new();
            while let Some(row) = rows.next()? {
                let signature = match (row.get::<_, Option<i64>>(4)?, row.get::<_, Option<i64>>(5)?)
                {
                    (Some(size), Some(modified)) => {
                        Some(signature_from_sql(size, modified, row.get(6)?)?)
                    }
                    _ => None,
                };
                items.push(JobItem {
                    key: row.get(0)?,
                    file: optional_blob16(row, 1, "job_items.file_id")?
                        .map(MediaFileId::from_bytes),
                    outcome: outcome_from_sql(&row.get::<_, String>(2)?)?,
                    detail: row.get(3)?,
                    signature,
                });
            }
            Ok(items)
        })
    }

    /// Queued and running jobs, oldest first. After an unclean exit these are the jobs to resume or
    /// cancel.
    pub fn unfinished_jobs(&self) -> Result<Vec<JobRecord>, StorageError> {
        self.read(|tx| {
            let mut statement = tx.prepare(&format!(
                "{JOB_COLUMNS} WHERE state IN ('queued', 'running') ORDER BY created_at_ms, id"
            ))?;
            let mut rows = statement.query([])?;
            let mut jobs = Vec::new();
            while let Some(row) = rows.next()? {
                jobs.push(job_from_row(row)?);
            }
            Ok(jobs)
        })
    }
}

const JOB_COLUMNS: &str =
    "SELECT id, kind, state, failure_reason, progress_done, progress_total FROM jobs";

fn load_job(tx: &Transaction<'_>, job: JobId) -> Result<Option<JobRecord>, StorageError> {
    let mut statement = tx.prepare(&format!("{JOB_COLUMNS} WHERE id = ?1"))?;
    let mut rows = statement.query([job.to_bytes()])?;
    rows.next()?.map(job_from_row).transpose()
}

fn job_from_row(row: &Row<'_>) -> Result<JobRecord, StorageError> {
    let progress = Progress {
        done: sql_to_u64(row.get(4)?, "jobs.progress_done")?,
        total: row
            .get::<_, Option<i64>>(5)?
            .map(|total| sql_to_u64(total, "jobs.progress_total"))
            .transpose()?,
    };
    Ok(JobRecord {
        id: JobId::from_bytes(blob16(row, 0, "jobs.id")?),
        kind: kind_from_sql(&row.get::<_, String>(1)?)?,
        state: state_from_sql(&row.get::<_, String>(2)?, row.get(3)?, progress)?,
        progress,
    })
}

fn kind_to_sql(kind: JobKind) -> &'static str {
    match kind {
        JobKind::LibraryIndex => "library-index",
        JobKind::ReplayGainScan => "replaygain-scan",
        JobKind::TagWrite => "tag-write",
        JobKind::ArtworkImport => "artwork-import",
        JobKind::ProviderSync => "provider-sync",
        JobKind::Backup => "backup",
    }
}

fn kind_from_sql(text: &str) -> Result<JobKind, StorageError> {
    Ok(match text {
        "library-index" => JobKind::LibraryIndex,
        "replaygain-scan" => JobKind::ReplayGainScan,
        "tag-write" => JobKind::TagWrite,
        "artwork-import" => JobKind::ArtworkImport,
        "provider-sync" => JobKind::ProviderSync,
        "backup" => JobKind::Backup,
        _ => {
            return Err(StorageError::InvalidStoredValue {
                column: "jobs.kind",
            });
        }
    })
}

fn state_to_sql(state: &JobState) -> (&'static str, Option<&str>) {
    match state {
        JobState::Queued => ("queued", None),
        JobState::Running(_) => ("running", None),
        JobState::Completed => ("completed", None),
        JobState::Incomplete => ("incomplete", None),
        JobState::Failed { reason } => ("failed", Some(reason)),
        JobState::Stale => ("stale", None),
        JobState::Cancelled => ("cancelled", None),
    }
}

fn state_from_sql(
    text: &str,
    reason: Option<String>,
    progress: Progress,
) -> Result<JobState, StorageError> {
    Ok(match (text, reason) {
        ("queued", None) => JobState::Queued,
        ("running", None) => JobState::Running(progress),
        ("completed", None) => JobState::Completed,
        ("incomplete", None) => JobState::Incomplete,
        ("failed", Some(reason)) => JobState::Failed { reason },
        ("stale", None) => JobState::Stale,
        ("cancelled", None) => JobState::Cancelled,
        _ => {
            return Err(StorageError::InvalidStoredValue {
                column: "jobs.state",
            });
        }
    })
}

fn outcome_to_sql(outcome: ItemOutcome) -> &'static str {
    match outcome {
        ItemOutcome::Pending => "pending",
        ItemOutcome::Done => "done",
        ItemOutcome::Skipped => "skipped",
        ItemOutcome::Failed => "failed",
    }
}

fn outcome_from_sql(text: &str) -> Result<ItemOutcome, StorageError> {
    match text {
        "pending" => Ok(ItemOutcome::Pending),
        "done" => Ok(ItemOutcome::Done),
        "skipped" => Ok(ItemOutcome::Skipped),
        "failed" => Ok(ItemOutcome::Failed),
        _ => Err(StorageError::InvalidStoredValue {
            column: "job_items.outcome",
        }),
    }
}
