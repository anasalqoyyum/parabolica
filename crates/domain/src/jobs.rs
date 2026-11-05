//! Background job kinds and their state machine.

use crate::ids::JobId;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum JobKind {
    LibraryIndex,
    ReplayGainScan,
    TagWrite,
    ArtworkImport,
    ProviderSync,
    Backup,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Progress {
    pub done: u64,
    pub total: Option<u64>,
}

/// Job lifecycle. `Completed` is the scanner's Ready state. A later change to a completed job's
/// inputs marks it `Stale`; rerunning creates a new job.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JobState {
    Queued,
    Running(Progress),
    Completed,
    /// Finished with skipped or failed items; results are not published as complete.
    Incomplete,
    Failed {
        reason: String,
    },
    Stale,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("job cannot move from {from:?} to {to:?}")]
pub struct InvalidJobTransition {
    pub from: JobState,
    pub to: JobState,
}

impl JobState {
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            JobState::Incomplete | JobState::Failed { .. } | JobState::Stale | JobState::Cancelled
        )
    }

    pub fn transition(&self, to: JobState) -> Result<JobState, InvalidJobTransition> {
        let allowed = matches!(
            (self, &to),
            (JobState::Queued, JobState::Running(_) | JobState::Cancelled)
                | (
                    JobState::Running(_),
                    JobState::Running(_)
                        | JobState::Completed
                        | JobState::Incomplete
                        | JobState::Failed { .. }
                        | JobState::Cancelled,
                )
                | (JobState::Completed, JobState::Stale)
        );
        if allowed {
            Ok(to)
        } else {
            Err(InvalidJobTransition {
                from: self.clone(),
                to,
            })
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobUpdate {
    pub job: JobId,
    pub kind: JobKind,
    pub state: JobState,
}
