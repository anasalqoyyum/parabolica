use std::io;
use std::path::PathBuf;

use parabolica_domain::jobs::InvalidJobTransition;
use rusqlite::ErrorCode;

/// A failed storage request. Messages name tables, columns and identifiers but never stored
/// values, so they are safe to log.
#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("the database file is damaged ({detail})")]
    Corrupt { detail: String },
    #[error("database request failed: {0}")]
    Sqlite(rusqlite::Error),
    #[error("storage is closed")]
    Closed,
    #[error("a storage write panicked and was rolled back")]
    WritePanicked,
    #[error("{0} is not stored")]
    NotFound(String),
    #[error("{0}")]
    Conflict(String),
    #[error("invalid {what}: {reason}")]
    Invalid {
        what: &'static str,
        reason: &'static str,
    },
    #[error(transparent)]
    JobTransition(#[from] InvalidJobTransition),
    #[error("stored {column} is not a valid value")]
    InvalidStoredValue { column: &'static str },
    #[error("file system operation failed")]
    Io(#[from] io::Error),
}

impl StorageError {
    pub(crate) fn invalid(what: &'static str, reason: &'static str) -> Self {
        StorageError::Invalid { what, reason }
    }

    /// Whether the database file itself is damaged, so the caller should offer recovery.
    pub fn is_corrupt(&self) -> bool {
        matches!(self, StorageError::Corrupt { .. })
    }
}

impl From<rusqlite::Error> for StorageError {
    fn from(error: rusqlite::Error) -> Self {
        match error.sqlite_error_code() {
            Some(ErrorCode::DatabaseCorrupt | ErrorCode::NotADatabase) => StorageError::Corrupt {
                detail: error.to_string(),
            },
            _ => StorageError::Sqlite(error),
        }
    }
}

/// Why a database could not be opened. Opening never overwrites or deletes an existing file; the
/// caller chooses one of [`OpenError::recovery_options`].
#[derive(Debug, thiserror::Error)]
pub enum OpenError {
    #[error("{} is open in another Parabolica window", path.display())]
    InUse { path: PathBuf },
    #[error("{} is damaged ({detail})", path.display())]
    Corrupt { path: PathBuf, detail: String },
    #[error("{} cannot be used by this version: {reason}", path.display())]
    Incompatible {
        path: PathBuf,
        reason: Incompatibility,
    },
    #[error("migration {migration} failed; the database was left unchanged")]
    MigrationFailed {
        migration: String,
        /// The copy taken before migrating, when the database already held data.
        backup: Option<PathBuf>,
        #[source]
        source: Box<StorageError>,
    },
    #[error("cannot open the database")]
    Io(#[from] io::Error),
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Incompatibility {
    #[error("it is not a Parabolica database")]
    NotParabolica,
    #[error("it was created by a newer version (unknown migrations: {})", unknown.join(", "))]
    NewerSchema { unknown: Vec<String> },
    #[error("applied migration {name} differs from this version's copy")]
    ModifiedMigration { name: String },
}

/// Actions the app offers when a database cannot be opened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoveryOption {
    /// Try again, for example after closing the other window.
    Retry,
    /// Copy the database files somewhere else without modifying them ([`crate::recovery::export_copy`]).
    ExportCopy,
    /// Rename the files aside and create an empty database ([`crate::recovery::move_aside`]).
    MoveAsideAndStartFresh,
}

impl OpenError {
    pub fn recovery_options(&self) -> &'static [RecoveryOption] {
        match self {
            OpenError::InUse { .. } | OpenError::Io(_) => &[RecoveryOption::Retry],
            OpenError::Corrupt { .. }
            | OpenError::Incompatible { .. }
            | OpenError::MigrationFailed { .. } => &[
                RecoveryOption::ExportCopy,
                RecoveryOption::MoveAsideAndStartFresh,
            ],
        }
    }
}
