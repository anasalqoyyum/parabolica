//! Services an account connects to.

use std::fmt;

use parabolica_domain::provider::ProviderId;

/// A service the user can connect an account to. Catalog providers have a [`ProviderId`];
/// scrobbling is an account without a catalog.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ServiceId {
    Provider(ProviderId),
    Scrobbling,
}

impl ServiceId {
    pub const YOUTUBE_MUSIC: Self = Self::Provider(ProviderId::YouTubeMusic);
    pub const DISCOGS: Self = Self::Provider(ProviderId::Discogs);

    /// Stable key used in OS credential names.
    pub const fn key(self) -> &'static str {
        match self {
            Self::Provider(provider) => provider.key(),
            Self::Scrobbling => "scrobbling",
        }
    }

    pub const fn name(self) -> &'static str {
        match self {
            Self::Provider(ProviderId::YouTubeMusic) => "YouTube Music",
            Self::Provider(ProviderId::Discogs) => "Discogs",
            Self::Scrobbling => "Scrobbling",
        }
    }
}

impl fmt::Display for ServiceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// The account a credential belongs to. `id` is the service's stable account identifier and is
/// what a reconnect compares; `display_name` is what the user sees.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountIdentity {
    pub id: String,
    pub display_name: String,
}
