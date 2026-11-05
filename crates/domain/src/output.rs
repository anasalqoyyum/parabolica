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
