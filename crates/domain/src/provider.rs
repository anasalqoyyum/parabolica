//! Provider identity and capabilities.

use std::collections::BTreeMap;
use std::fmt;
use std::time::Duration;

/// External services the app integrates with. Adding one is a coordinated domain change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ProviderId {
    YouTubeMusic,
    Discogs,
}

impl ProviderId {
    /// Stable key used in storage and exports.
    pub const fn key(self) -> &'static str {
        match self {
            ProviderId::YouTubeMusic => "youtube-music",
            ProviderId::Discogs => "discogs",
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        [ProviderId::YouTubeMusic, ProviderId::Discogs]
            .into_iter()
            .find(|provider| provider.key() == key)
    }
}

/// A provider's own identifier for an item, kept verbatim.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProviderItemId(pub String);

/// Provider items are identified by provider and provider ID, never by title.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProviderItemRef {
    pub provider: ProviderId,
    pub item: ProviderItemId,
}

impl fmt::Display for ProviderItemRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.provider.key(), self.item.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Capability {
    Search,
    LibraryRead,
    PlaylistRead,
    PlaylistWrite,
    Likes,
    Radio,
    Lyrics,
    Playback,
    Seek,
    QualityInspection,
    /// The provider hands the native engine decodable media, so ReplayGain, device selection and
    /// the signal path apply. Without it the provider owns decoding and output.
    AppOwnedPcm,
    BackgroundPlayback,
    Gapless,
    ReleaseLookup,
    ArtworkLookup,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UnavailableReason {
    NotSupported,
    SignedOut,
    SessionExpired,
    Offline,
    RegionRestricted,
    RateLimited { retry_after: Option<Duration> },
    Blocked(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Availability {
    Available,
    Unavailable(UnavailableReason),
}

/// Where a provider item's audio is decoded and sent to a device.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaybackOwner {
    NativeEngine,
    Provider(ProviderId),
}

/// Capabilities reported by one provider session. A capability that was not reported is
/// `NotSupported`; callers never assume one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderCapabilities {
    provider: ProviderId,
    reported: BTreeMap<Capability, Availability>,
}

impl ProviderCapabilities {
    pub fn new(provider: ProviderId) -> Self {
        Self {
            provider,
            reported: BTreeMap::new(),
        }
    }

    pub fn provider(&self) -> ProviderId {
        self.provider
    }

    pub fn set(&mut self, capability: Capability, availability: Availability) -> &mut Self {
        self.reported.insert(capability, availability);
        self
    }

    pub fn availability(&self, capability: Capability) -> Availability {
        self.reported
            .get(&capability)
            .cloned()
            .unwrap_or(Availability::Unavailable(UnavailableReason::NotSupported))
    }

    pub fn is_available(&self, capability: Capability) -> bool {
        self.availability(capability) == Availability::Available
    }

    pub fn playback_owner(&self) -> PlaybackOwner {
        if self.is_available(Capability::AppOwnedPcm) {
            PlaybackOwner::NativeEngine
        } else {
            PlaybackOwner::Provider(self.provider)
        }
    }
}
