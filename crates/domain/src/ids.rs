//! Stable internal identities.
//!
//! Every persisted entity gets an app-generated UUIDv7. Identities never derive from paths, titles
//! or provider data, so two editions with the same title stay distinct and a moved file can keep
//! its identity. Storage persists the 16 raw bytes; logs and exports use the prefixed text form.

use std::fmt;
use std::str::FromStr;

use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum IdParseError {
    #[error("expected an identifier starting with `{expected}_`")]
    WrongPrefix { expected: &'static str },
    #[error("identifier body is not a 32-digit hexadecimal UUID")]
    InvalidBody,
}

macro_rules! stable_id {
    ($(#[$meta:meta])* $name:ident, $prefix:literal) => {
        $(#[$meta])*
        #[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(Uuid);

        impl $name {
            pub const PREFIX: &'static str = $prefix;

            pub fn generate() -> Self {
                Self(Uuid::now_v7())
            }

            pub const fn from_bytes(bytes: [u8; 16]) -> Self {
                Self(Uuid::from_bytes(bytes))
            }

            pub const fn to_bytes(self) -> [u8; 16] {
                *self.0.as_bytes()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}_{}", Self::PREFIX, self.0.simple())
            }
        }

        impl fmt::Debug for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                fmt::Display::fmt(self, f)
            }
        }

        impl FromStr for $name {
            type Err = IdParseError;

            fn from_str(text: &str) -> Result<Self, Self::Err> {
                let body = text
                    .strip_prefix(Self::PREFIX)
                    .and_then(|rest| rest.strip_prefix('_'))
                    .ok_or(IdParseError::WrongPrefix { expected: Self::PREFIX })?;
                if body.len() != 32 {
                    return Err(IdParseError::InvalidBody);
                }
                Uuid::try_parse(body).map(Self).map_err(|_| IdParseError::InvalidBody)
            }
        }
    };
}

stable_id!(
    /// A folder the user added to the library.
    LibraryRootId,
    "root"
);
stable_id!(
    /// A physical file on disk, including a FLAC image referenced by a CUE sheet.
    MediaFileId,
    "file"
);
stable_id!(
    /// A CUE-defined track: a frame range inside one physical file.
    VirtualTrackId,
    "vtrk"
);
stable_id!(
    /// A logical recording. Local files and provider items link to it only after confirmation.
    TrackId,
    "trk"
);
stable_id!(
    /// One album edition. Same-titled editions get different release IDs.
    ReleaseId,
    "rel"
);
stable_id!(ArtistId, "art");
stable_id!(ArtworkId, "img");
stable_id!(PlaylistId, "pl");
stable_id!(
    /// One row in a playlist. Duplicate tracks keep separate entry IDs.
    PlaylistEntryId,
    "ple"
);
stable_id!(
    /// One row in the play queue. Survives reordering, shuffle and restart.
    QueueEntryId,
    "qe"
);
stable_id!(
    /// A background job such as an index pass, ReplayGain scan or tag write.
    JobId,
    "job"
);
