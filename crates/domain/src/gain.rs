//! ReplayGain values, modes and app volume.

pub mod analysis;

/// A finite gain in decibels.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub struct Decibels(f32);

impl Decibels {
    pub const ZERO: Self = Self(0.0);

    pub fn new(db: f32) -> Option<Self> {
        db.is_finite().then_some(Self(db))
    }

    pub fn get(self) -> f32 {
        self.0
    }
}

/// A unitless linear peak amplitude (1.0 is full scale). Never stored in dB.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub struct LinearPeak(f32);

impl LinearPeak {
    pub fn new(peak: f32) -> Option<Self> {
        (peak.is_finite() && peak >= 0.0).then_some(Self(peak))
    }

    pub fn get(self) -> f32 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PeakMethod {
    Sample,
    TruePeak,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GainMeasurement {
    pub gain: Decibels,
    pub peak: Option<LinearPeak>,
    pub peak_method: PeakMethod,
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct StoredGains {
    pub track: Option<GainMeasurement>,
    pub album: Option<GainMeasurement>,
}

/// The user's ReplayGain choice. Off until peak protection exists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ReplayGainMode {
    #[default]
    Off,
    Track,
    Album,
    Auto,
}

/// How the current queue is being played, used to resolve [`ReplayGainMode::Auto`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaybackContext {
    /// Tracks of one release in release order.
    OrderedAlbum,
    /// Shuffled, mixed-release or otherwise non-album playback.
    Mixed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EffectiveGainMode {
    Off,
    Track,
    Album,
}

impl ReplayGainMode {
    pub fn resolve(self, context: PlaybackContext) -> EffectiveGainMode {
        match (self, context) {
            (ReplayGainMode::Off, _) => EffectiveGainMode::Off,
            (ReplayGainMode::Track, _) | (ReplayGainMode::Auto, PlaybackContext::Mixed) => {
                EffectiveGainMode::Track
            }
            (ReplayGainMode::Album, _) | (ReplayGainMode::Auto, PlaybackContext::OrderedAlbum) => {
                EffectiveGainMode::Album
            }
        }
    }
}

/// The measurement the effective mode selects for one item.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum GainSelection {
    Bypassed,
    Measured {
        mode: EffectiveGainMode,
        measurement: GainMeasurement,
    },
    /// The mode's measurement is missing. Plays at 0 dB and shows Not scanned; album mode never
    /// substitutes track gain.
    NotScanned {
        mode: EffectiveGainMode,
    },
}

impl GainSelection {
    pub fn select(mode: EffectiveGainMode, gains: &StoredGains) -> Self {
        let measurement = match mode {
            EffectiveGainMode::Off => return GainSelection::Bypassed,
            EffectiveGainMode::Track => gains.track,
            EffectiveGainMode::Album => gains.album,
        };
        match measurement {
            Some(measurement) => GainSelection::Measured { mode, measurement },
            None => GainSelection::NotScanned { mode },
        }
    }
}

/// Gain the engine applies, with the requested value kept for display when protection lowers it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AppliedGain {
    pub requested: Decibels,
    pub applied: Decibels,
}

impl AppliedGain {
    pub const UNITY: Self = Self {
        requested: Decibels::ZERO,
        applied: Decibels::ZERO,
    };

    pub fn limited_by_protection(&self) -> bool {
        self.applied.get() < self.requested.get()
    }
}

/// App volume as linear amplitude in `[0, 1]`. Separate from Windows and hardware volume.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub struct Volume(f32);

impl Volume {
    pub const UNITY: Self = Self(1.0);

    pub fn new(linear: f32) -> Option<Self> {
        (0.0..=1.0).contains(&linear).then_some(Self(linear))
    }

    pub fn get(self) -> f32 {
        self.0
    }
}
