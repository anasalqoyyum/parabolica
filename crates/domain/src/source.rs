//! Playable sources and their resolution.
//!
//! A [`PlayableSource`] is what a queue or playlist entry refers to. Resolution turns it into a
//! [`ResolvedSource`]: the physical file and frame span, or the provider stream, a backend opens.
//! The virtual identity stays on the queue entry while the backend sees the physical file.

use std::path::PathBuf;
use std::time::SystemTime;

use crate::cue::VirtualTrackRange;
use crate::format::DeliveryFormat;
use crate::ids::{MediaFileId, VirtualTrackId};
use crate::provider::{PlaybackOwner, ProviderItemRef};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum PlayableSource {
    LocalFile(MediaFileId),
    CueTrack(VirtualTrackId),
    Provider(ProviderItemRef),
}

impl PlayableSource {
    pub fn is_local(&self) -> bool {
        !matches!(self, PlayableSource::Provider(_))
    }
}

/// Frames of a physical file that belong to the resolved source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FrameSpan {
    Whole,
    Range(std::ops::Range<u64>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolvedSource {
    /// A local file or a CUE range of one, decoded by the native engine.
    Local {
        file: MediaFileId,
        path: PathBuf,
        span: FrameSpan,
    },
    /// Provider media the native engine fetches and decodes.
    ProviderPcm {
        item: ProviderItemRef,
        delivery: DeliveryFormat,
        expires_at: Option<SystemTime>,
    },
    /// The provider (for example a browser session) decodes and outputs the audio itself.
    ProviderManaged {
        item: ProviderItemRef,
        delivery: DeliveryFormat,
    },
}

impl ResolvedSource {
    pub fn local_file(file: MediaFileId, path: PathBuf) -> Self {
        ResolvedSource::Local {
            file,
            path,
            span: FrameSpan::Whole,
        }
    }

    /// Resolve a CUE track to its image file and exact frame range.
    pub fn cue_track(range: &VirtualTrackRange, image_path: PathBuf) -> Self {
        ResolvedSource::Local {
            file: range.image,
            path: image_path,
            span: FrameSpan::Range(range.frames.clone()),
        }
    }

    pub fn playback_owner(&self) -> PlaybackOwner {
        match self {
            ResolvedSource::Local { .. } | ResolvedSource::ProviderPcm { .. } => {
                PlaybackOwner::NativeEngine
            }
            ResolvedSource::ProviderManaged { item, .. } => PlaybackOwner::Provider(item.provider),
        }
    }
}
