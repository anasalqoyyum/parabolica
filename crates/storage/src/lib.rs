//! SQLite storage. One writer thread owns the write connection; see `docs/ARCHITECTURE.md`.

pub mod backup;
