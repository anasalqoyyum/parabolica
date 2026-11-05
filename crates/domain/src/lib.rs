//! Shared contracts for Parabolica.
//!
//! This crate has no UI, audio-device, database or network dependencies. Feature crates and the
//! desktop app exchange these types; see `docs/ARCHITECTURE.md` for the rules they encode.

pub mod control;
pub mod cue;
pub mod dispatch;
pub mod files;
pub mod format;
pub mod gain;
pub mod ids;
pub mod jobs;
pub mod output;
pub mod playback;
pub mod provider;
pub mod queue;
pub mod signal_path;
pub mod source;
pub mod tags;
