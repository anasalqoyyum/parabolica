//! SQLite storage. One writer thread owns the write connection; see `docs/ARCHITECTURE.md`.
//!
//! [`Storage`] is the only way into the database. Its methods are typed repository operations,
//! each one transaction; feature issues add their tables as `migrations/NNNN_MM_slug.sql` and their
//! operations as `impl Storage` blocks in their own module.

pub mod backup;
mod codec;
pub mod collection;
mod database;
mod error;
pub mod jobs;
pub mod library;
mod migrate;
pub mod playlists;
pub mod preferences;
pub mod queue;
pub mod recovery;
pub mod session;
pub mod sources;

pub use database::Storage;
pub use error::{Incompatibility, OpenError, RecoveryOption, StorageError};
