//! Output backends, devices and the negotiated stream.

use std::time::Duration;

use crate::format::DeliveryFormat;
use crate::format::{OutputFormat, PcmFormat};
use crate::provider::{PlaybackOwner, ProviderItemRef};
use crate::source::ResolvedSource;

/// Native output backends. macOS and Linux ports add their own variants.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum OutputBackend {
    WasapiShared,
    WasapiExclusive,
    Asio,
}

impl OutputBackend {
    /// Whether the Windows shared mixer and session volume sit between the app and the device.
    pub fn uses_system_mixer(self) -> bool {
        matches!(self, OutputBackend::WasapiShared)
    }
}

/// A device as enumerated by one backend: a WASAPI endpoint ID or an ASIO driver CLSID.
/// The same hardware under two backends has two identities.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct OutputDeviceId {
    pub backend: OutputBackend,
    pub stable_id: String,
}

/// Exclusive output requests the source rate or this explicitly selected fixed rate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SampleRatePolicy {
    #[default]
    FollowSource,
    Fixed(crate::format::SampleRate),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputSelection {
    pub backend: OutputBackend,
    /// None resolves the Windows default once. Subsequent opens retain that endpoint.
    pub device: Option<OutputDeviceId>,
    pub rate: SampleRatePolicy,
    pub asio: AsioSelection,
}

impl Default for OutputSelection {
    fn default() -> Self {
        Self {
            backend: OutputBackend::WasapiShared,
            device: None,
            rate: SampleRatePolicy::FollowSource,
            asio: AsioSelection::default(),
        }
    }
}

impl OutputSelection {
    pub fn needs_new_stream(
        &self,
        source: &crate::format::SourceFormat,
        output: &NegotiatedOutput,
    ) -> bool {
        if self.backend == OutputBackend::Asio {
            let rate = match self.rate {
                SampleRatePolicy::FollowSource => source.rate,
                SampleRatePolicy::Fixed(rate) => rate,
            };
            return rate != output.format.rate
                || source.channels.get() > output.format.channels.get();
        }
        if self.backend != OutputBackend::WasapiExclusive {
            return false;
        }
        let rate = match self.rate {
            SampleRatePolicy::FollowSource => source.rate,
            SampleRatePolicy::Fixed(rate) => rate,
        };
        let precision = match (source.sample, output.format.sample) {
            (
                Some(crate::format::SourceSampleFormat::Int { bits }),
                crate::format::OutputSampleFormat::Int { valid_bits, .. },
            ) => bits > valid_bits,
            (
                Some(crate::format::SourceSampleFormat::Float { .. }),
                crate::format::OutputSampleFormat::Int { .. },
            ) => true,
            (
                Some(crate::format::SourceSampleFormat::Int { bits }),
                crate::format::OutputSampleFormat::Float { bits: 32 },
            ) => bits > 24,
            (
                Some(crate::format::SourceSampleFormat::Float { bits: source_bits }),
                crate::format::OutputSampleFormat::Float { bits },
            ) => source_bits > bits,
            _ => false,
        };
        rate != output.format.rate || source.channels != output.format.channels || precision
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioEndpoint {
    pub available: bool,
    pub stable_id: String,
    pub name: String,
    pub is_default: bool,
    pub mix_format: Option<OutputFormat>,
    /// Exact stereo formats accepted by IsFormatSupported in Exclusive mode.
    pub exclusive_formats: Vec<OutputFormat>,
    pub capability_error: Option<String>,
}

/// ASIO channel indices are zero-based and belong to the selected driver only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AsioSelection {
    pub channels: Vec<u16>,
    /// None uses the driver's current preferred size on every open.
    pub buffer_frames: Option<u32>,
}

impl Default for AsioSelection {
    fn default() -> Self {
        Self {
            channels: vec![0, 1],
            buffer_frames: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AsioDriver {
    pub stable_id: String,
    pub name: String,
    pub architecture: String,
    pub compatible: bool,
    pub library_path: String,
    pub unavailable_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AsioChannel {
    pub index: u16,
    pub name: String,
    pub sample_type: i32,
    pub sample: Option<crate::format::OutputSampleFormat>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AsioCapabilities {
    pub device: OutputDeviceId,
    pub driver_name: String,
    pub driver_version: i32,
    pub outputs: Vec<AsioChannel>,
    /// Individually queried rates, not an exhaustive range or a promise of open success.
    pub rates: Vec<crate::format::SampleRate>,
    pub current_rate: Option<crate::format::SampleRate>,
    pub buffer: AsioBufferSizes,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AsioBufferSizes {
    pub min: u32,
    pub max: u32,
    pub preferred: u32,
    /// -1 means powers of two; 0 means a fixed size; positive values are frame increments.
    pub granularity: i32,
}

impl AsioBufferSizes {
    pub fn supports(self, frames: u32) -> bool {
        if frames < self.min || frames > self.max {
            return false;
        }
        match self.granularity {
            -1 => frames.is_power_of_two(),
            0 => frames == self.preferred,
            step if step > 0 => (frames - self.min).is_multiple_of(step as u32),
            _ => false,
        }
    }
}

/// Who controls the buffer size. ASIO drivers often own it; the app must then report it rather
/// than offer an ineffective control.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BufferOwner {
    App,
    Driver,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BufferConfig {
    pub frames: u32,
    pub owner: BufferOwner,
}

/// The stream the backend actually opened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NegotiatedOutput {
    pub device: OutputDeviceId,
    pub device_name: String,
    pub format: OutputFormat,
    /// Device output channel for each internal PCM channel, in PCM channel order.
    pub channel_map: Vec<u16>,
    pub buffer: BufferConfig,
    /// Output latency as reported by the backend or driver. Absent when it was not reported.
    pub reported_latency: Option<Duration>,
}

impl NegotiatedOutput {
    /// Buffer length implied by frames and rate. This is not round-trip or output latency.
    pub fn buffer_duration(&self) -> Duration {
        Duration::from_secs_f64(f64::from(self.buffer.frames) / f64::from(self.format.rate.hz()))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RouteError {
    #[error("the provider owns playback for this source; it cannot use a native output stream")]
    ProviderOwnsPlayback,
    #[error("only provider-managed sources can bypass the native engine")]
    NotProviderManaged,
    #[error("channel map covers {mapped} channels but internal PCM has {pcm}")]
    ChannelMapMismatch { mapped: usize, pcm: u16 },
}

/// How the current item reaches a device. Exactly one route produces sound at a time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlaybackRoute {
    Native {
        source: ResolvedSource,
        pcm: PcmFormat,
        output: NegotiatedOutput,
    },
    ProviderManaged {
        item: ProviderItemRef,
        delivery: DeliveryFormat,
    },
}

impl PlaybackRoute {
    pub fn native(
        source: ResolvedSource,
        pcm: PcmFormat,
        output: NegotiatedOutput,
    ) -> Result<Self, RouteError> {
        if let PlaybackOwner::Provider(_) = source.playback_owner() {
            return Err(RouteError::ProviderOwnsPlayback);
        }
        if output.channel_map.len() != usize::from(pcm.channels.get()) {
            return Err(RouteError::ChannelMapMismatch {
                mapped: output.channel_map.len(),
                pcm: pcm.channels.get(),
            });
        }
        Ok(PlaybackRoute::Native {
            source,
            pcm,
            output,
        })
    }

    pub fn provider_managed(source: ResolvedSource) -> Result<Self, RouteError> {
        match source {
            ResolvedSource::ProviderManaged { item, delivery } => {
                Ok(PlaybackRoute::ProviderManaged { item, delivery })
            }
            _ => Err(RouteError::NotProviderManaged),
        }
    }

    /// Whether native ReplayGain, app volume, device selection and exclusive/ASIO modes apply.
    pub fn native_controls_apply(&self) -> bool {
        matches!(self, PlaybackRoute::Native { .. })
    }

    pub fn requires_resampling(&self) -> bool {
        match self {
            PlaybackRoute::Native { pcm, output, .. } => pcm.rate != output.format.rate,
            PlaybackRoute::ProviderManaged { .. } => false,
        }
    }
}
