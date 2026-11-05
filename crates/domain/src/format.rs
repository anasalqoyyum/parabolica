//! Source, internal PCM and output formats.
//!
//! The three are deliberately separate types. A FLAC file's bit depth says nothing about the
//! engine's internal representation, and neither says what the device stream negotiated.

use std::num::{NonZeroU16, NonZeroU32};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SampleRate(NonZeroU32);

impl SampleRate {
    pub const fn new(hz: u32) -> Option<Self> {
        match NonZeroU32::new(hz) {
            Some(hz) => Some(Self(hz)),
            None => None,
        }
    }

    pub const fn hz(self) -> u32 {
        self.0.get()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ChannelCount(NonZeroU16);

impl ChannelCount {
    pub const STEREO: Self = Self(NonZeroU16::new(2).expect("2 is non-zero"));

    pub const fn new(channels: u16) -> Option<Self> {
        match NonZeroU16::new(channels) {
            Some(channels) => Some(Self(channels)),
            None => None,
        }
    }

    pub const fn get(self) -> u16 {
        self.0.get()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Codec {
    Flac,
    Alac,
    Wav,
    Mp3,
    Aac,
    Vorbis,
    Opus,
    Other(String),
}

impl Codec {
    pub fn is_lossless(&self) -> bool {
        matches!(self, Codec::Flac | Codec::Alac | Codec::Wav)
    }
}

/// Sample representation stored in the source, when the codec has one. Lossy codecs do not.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SourceSampleFormat {
    Int { bits: u8 },
    Float { bits: u8 },
}

/// What the decoder reports about a local file or what a provider reports about a stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceFormat {
    pub codec: Codec,
    pub sample: Option<SourceSampleFormat>,
    pub rate: SampleRate,
    pub channels: ChannelCount,
}

/// Sample type of the engine's internal PCM.
///
/// `f64` holds every integer sample of up to 32 bits exactly, so decoding FLAC of any supported
/// depth into the engine loses nothing before an explicit processing stage.
pub type PcmSample = f64;

/// Interleaved internal PCM, always [`PcmSample`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PcmFormat {
    pub rate: SampleRate,
    pub channels: ChannelCount,
}

/// Scale a signed integer sample of `bits` width to the engine's [-1.0, 1.0) range.
///
/// # Panics
///
/// Panics when `bits` is outside `1..=32`.
pub fn int_to_pcm(sample: i32, bits: u8) -> PcmSample {
    assert!((1..=32).contains(&bits), "unsupported integer width {bits}");
    sample as PcmSample / (1u64 << (bits - 1)) as PcmSample
}

/// Inverse of [`int_to_pcm`]. Returns `None` when the value is not an exact `bits`-wide sample.
pub fn pcm_to_int_exact(sample: PcmSample, bits: u8) -> Option<i32> {
    if !(1..=32).contains(&bits) {
        return None;
    }
    let scaled = sample * (1u64 << (bits - 1)) as PcmSample;
    let min = -((1i64 << (bits - 1)) as PcmSample);
    let max = ((1i64 << (bits - 1)) - 1) as PcmSample;
    (scaled.fract() == 0.0 && scaled >= min && scaled <= max).then_some(scaled as i32)
}

/// Sample type of a negotiated device stream. ASIO drivers commonly report 32-bit containers with
/// 24 valid bits, which `Int { container_bits: 32, valid_bits: 24 }` represents.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OutputSampleFormat {
    Int { container_bits: u8, valid_bits: u8 },
    Float { bits: u8 },
}

/// The stream format returned by the active backend, never the one the app requested.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct OutputFormat {
    pub sample: OutputSampleFormat,
    pub rate: SampleRate,
    pub channels: ChannelCount,
}

/// What a provider reports about stream delivery. Every field can be unknown.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DeliveryFormat {
    pub codec: Option<Codec>,
    pub bitrate_kbps: Option<u32>,
    pub rate: Option<SampleRate>,
    pub channels: Option<ChannelCount>,
}
