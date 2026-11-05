//! Playback controller commands, events and snapshots.

use std::time::{Duration, Instant};

use crate::control::{Generation, Versioned};
use crate::format::SampleRate;
use crate::gain::{GainSelection, ReplayGainMode, Volume};
use crate::ids::QueueEntryId;
use crate::output::{OutputDeviceId, PlaybackRoute};
use crate::signal_path::SignalPath;

#[derive(Debug, Clone, PartialEq)]
pub enum PlaybackCommand {
    Play(QueueEntryId),
    Pause,
    Resume,
    Stop,
    Seek(Duration),
    Next,
    Previous,
    SetVolume(Volume),
    SetReplayGainMode(ReplayGainMode),
    SelectOutput(OutputDeviceId),
}

impl PlaybackCommand {
    /// Commands that make in-flight loads, seeks and provider responses obsolete.
    pub fn supersedes_pending_work(&self) -> bool {
        matches!(
            self,
            PlaybackCommand::Play(_)
                | PlaybackCommand::Stop
                | PlaybackCommand::Seek(_)
                | PlaybackCommand::Next
                | PlaybackCommand::Previous
                | PlaybackCommand::SelectOutput(_)
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportState {
    Stopped,
    Loading,
    Playing,
    Paused,
    /// Waiting for the user, for example after the selected output device disappeared.
    Blocked,
}

/// Playback position captured by the engine outside the audio callback.
///
/// The UI extrapolates from the stamp instead of counting its own frames, so the displayed
/// position does not drift with the UI frame rate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PositionStamp {
    pub frame: u64,
    pub rate: SampleRate,
    pub captured_at: Instant,
    pub advancing: bool,
}

impl PositionStamp {
    pub fn position_at(&self, now: Instant) -> Duration {
        let base = Duration::from_secs_f64(self.frame as f64 / f64::from(self.rate.hz()));
        if self.advancing {
            base + now.saturating_duration_since(self.captured_at)
        } else {
            base
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct PlaybackSnapshot {
    pub generation: Generation,
    pub transport: TransportState,
    pub current: Option<QueueEntryId>,
    pub position: Option<PositionStamp>,
    pub duration: Option<Duration>,
    pub route: Option<PlaybackRoute>,
    pub signal_path: Option<SignalPath>,
    pub gain: Option<GainSelection>,
    pub volume: Volume,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PlaybackEvent {
    Snapshot(Box<Versioned<PlaybackSnapshot>>),
    Position(PositionStamp),
    /// Total underruns since the stream opened, read from the callback's counter.
    Underruns {
        total: u64,
    },
    OutputLost {
        device: OutputDeviceId,
    },
    EntryFailed {
        entry: QueueEntryId,
        reason: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PlaybackEventKey {
    Snapshot,
    Position,
    Underruns,
}

impl crate::dispatch::Coalesce for PlaybackEvent {
    type Key = PlaybackEventKey;

    fn coalesce_key(&self) -> Option<Self::Key> {
        match self {
            PlaybackEvent::Snapshot(_) => Some(PlaybackEventKey::Snapshot),
            PlaybackEvent::Position(_) => Some(PlaybackEventKey::Position),
            PlaybackEvent::Underruns { .. } => Some(PlaybackEventKey::Underruns),
            PlaybackEvent::OutputLost { .. } | PlaybackEvent::EntryFailed { .. } => None,
        }
    }
}
