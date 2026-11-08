//! Playback controller, decoder workers and the PCM ring buffer.
//!
//! [`PlaybackEngine`] runs a controller thread that owns the transport state. A decoder worker
//! decodes the current file, converts its rate once if the device runs at another rate
//! ([`resample`]) and writes frames in the device's channel layout into a lock-free ring buffer.
//! The backend's device thread pulls them through [`StreamRenderer`], which applies app volume
//! and converts to the device sample type. ReplayGain, EQ, crossfade and other effects are off.
//!
//! Every sequence replacement, seek and stop starts a new generation. The worker flushes the ring
//! buffer before writing, and the controller drops worker reports from older generations, so a slow
//! seek or load cannot change a newly selected file. Compatible sequence entries share the
//! renderer, ring, output stream and converter history.

mod controller;
mod output;
mod render;
mod resample;
#[cfg(test)]
mod tests;
mod worker;

use std::path::PathBuf;

use parabolica_domain::control::{Generation, Versioned};
use parabolica_domain::dispatch::Coalesce;
use parabolica_domain::format::SourceFormat;
use parabolica_domain::ids::QueueEntryId;
use parabolica_domain::output::{NegotiatedOutput, OutputDeviceId};
use parabolica_domain::playback::{PlaybackSnapshot, PositionStamp};

pub use controller::PlaybackEngine;
pub use output::{NoOutput, Output, OutputError, OutputStream};
#[cfg(all(test, windows, feature = "asio"))]
pub(crate) use render::StreamShared;
pub use render::{OutputSample, StreamRenderer};

use crate::decode::SourceInfo;
use symphonia::core::audio::Channels;

/// Engine state for the player and the signal-path inspector.
#[derive(Debug, Clone, PartialEq)]
pub struct EngineSnapshot {
    pub playback: PlaybackSnapshot,
    pub path: Option<PathBuf>,
    /// What the decoder reports about the current file, once it has opened.
    pub source: Option<SourceInfo>,
    /// The open device stream, also present between tracks.
    pub output: Option<NegotiatedOutput>,
    pub underruns: u64,
    /// The last failure. Cleared when the next entry starts.
    pub failure: Option<PlaybackFailure>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaybackFailure {
    pub entry: Option<QueueEntryId>,
    pub path: Option<PathBuf>,
    pub error: PlaybackError,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PlaybackError {
    #[error("{0}")]
    Decode(String),
    #[error("{source_channels}-channel audio can't play on this {output_channels}-channel output")]
    ChannelLayout {
        source_channels: usize,
        output_channels: usize,
    },
    #[error("sample rate conversion failed: {0}")]
    Resampler(String),
    #[error(transparent)]
    Output(OutputError),
    #[error("only local files play in the native engine")]
    NotLocal,
    #[error("CUE tracks can't play yet")]
    CueRange,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EngineRequestId(pub(crate) u64);

#[derive(Debug, Clone, PartialEq)]
pub enum EngineEvent {
    AsioCapabilities {
        device: Option<OutputDeviceId>,
        result: Result<parabolica_domain::output::AsioCapabilities, OutputError>,
    },
    Snapshot(Box<Versioned<EngineSnapshot>>),
    Synchronized {
        request: EngineRequestId,
        snapshot: Box<EngineSnapshot>,
    },
    /// A fresh position stamp while playing, to correct extrapolation drift.
    Position(PositionStamp),
    /// Total underruns since the stream opened, read from the callback's counter.
    Underruns {
        total: u64,
    },
    /// Sequence selection, each rendered entry boundary and completion. Never coalesced.
    /// The initial `from: None` announces selection, including paused restoration.
    EntryBoundary {
        generation: Generation,
        from: Option<QueueEntryId>,
        to: Option<QueueEntryId>,
        /// Absolute frame on this generation's output clock.
        output_frame: u64,
    },
    /// The converter was reconfigured at this entry boundary. The device stream stays open.
    SourceFormatChanged {
        generation: Generation,
        entry: QueueEntryId,
        output_frame: u64,
        previous_layout: Channels,
        next_layout: Channels,
        previous: SourceFormat,
        next: SourceFormat,
    },
    OutputLost {
        device: OutputDeviceId,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EngineEventKey {
    Snapshot,
    Position,
    Underruns,
}

impl Coalesce for EngineEvent {
    type Key = EngineEventKey;

    fn coalesce_key(&self) -> Option<Self::Key> {
        match self {
            EngineEvent::Snapshot(_) => Some(EngineEventKey::Snapshot),
            EngineEvent::Position(_) => Some(EngineEventKey::Position),
            EngineEvent::Underruns { .. } => Some(EngineEventKey::Underruns),
            EngineEvent::AsioCapabilities { .. }
            | EngineEvent::Synchronized { .. }
            | EngineEvent::OutputLost { .. }
            | EngineEvent::EntryBoundary { .. }
            | EngineEvent::SourceFormatChanged { .. } => None,
        }
    }
}
