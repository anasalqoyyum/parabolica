//! Staged tag and artwork patches for whole-album writes.

use std::collections::BTreeSet;

use crate::files::FileSignature;
use crate::ids::{ArtworkId, MediaFileId, ReleaseId};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TagField {
    Title,
    Artist,
    Album,
    AlbumArtist,
    TrackNumber,
    TrackTotal,
    DiscNumber,
    DiscTotal,
    Date,
    Genre,
    Style,
    Label,
    CatalogNumber,
    Country,
    ReplayGainTrackGain,
    ReplayGainTrackPeak,
    ReplayGainAlbumGain,
    ReplayGainAlbumPeak,
    /// Any other field, by its container-native name (for example a Vorbis comment key).
    Custom(String),
}

/// One field change. Values are multi-valued; a single value is a one-element list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FieldEdit {
    Replace(Vec<String>),
    FillIfEmpty(Vec<String>),
    Remove,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldChange {
    pub field: TagField,
    pub edit: FieldEdit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PictureType {
    FrontCover,
    BackCover,
    Booklet,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArtworkDestination {
    Embed,
    BesideFiles,
    Both,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtworkChange {
    pub artwork: ArtworkId,
    pub picture_type: PictureType,
    pub destination: ArtworkDestination,
}

/// Changes for one physical file, valid only while the file still matches `reviewed`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TagPatch {
    pub target: MediaFileId,
    pub reviewed: FileSignature,
    pub fields: Vec<FieldChange>,
    pub artwork: Vec<ArtworkChange>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AlbumWriteError {
    #[error("{0} has more than one patch; a CUE image is one physical file and is written once")]
    DuplicateTarget(MediaFileId),
    #[error("{0} is both excluded and patched")]
    ExcludedTargetPatched(MediaFileId),
    #[error("the album write has no patches")]
    Empty,
}

/// An accepted album-level write. Per-file replacements are not atomic as a group.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AlbumWrite {
    release: ReleaseId,
    patches: Vec<TagPatch>,
    excluded: BTreeSet<MediaFileId>,
}

impl AlbumWrite {
    pub fn new(
        release: ReleaseId,
        patches: Vec<TagPatch>,
        excluded: BTreeSet<MediaFileId>,
    ) -> Result<Self, AlbumWriteError> {
        if patches.is_empty() {
            return Err(AlbumWriteError::Empty);
        }
        let mut seen = BTreeSet::new();
        for patch in &patches {
            if excluded.contains(&patch.target) {
                return Err(AlbumWriteError::ExcludedTargetPatched(patch.target));
            }
            if !seen.insert(patch.target) {
                return Err(AlbumWriteError::DuplicateTarget(patch.target));
            }
        }
        Ok(Self {
            release,
            patches,
            excluded,
        })
    }

    pub fn release(&self) -> ReleaseId {
        self.release
    }

    pub fn patches(&self) -> &[TagPatch] {
        &self.patches
    }

    pub fn excluded(&self) -> &BTreeSet<MediaFileId> {
        &self.excluded
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileWriteOutcome {
    Written {
        signature: FileSignature,
    },
    /// The file changed since review; nothing was written.
    ChangedSinceReview,
    /// A handle (for example active playback) prevents a safe replacement right now.
    Deferred,
    Failed {
        reason: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlbumWriteStatus {
    Complete,
    Partial,
    Failed,
}

impl AlbumWriteStatus {
    pub fn from_outcomes<'a>(outcomes: impl IntoIterator<Item = &'a FileWriteOutcome>) -> Self {
        let (mut written, mut other) = (0usize, 0usize);
        for outcome in outcomes {
            match outcome {
                FileWriteOutcome::Written { .. } => written += 1,
                _ => other += 1,
            }
        }
        match (written, other) {
            (_, 0) if written > 0 => AlbumWriteStatus::Complete,
            (0, _) => AlbumWriteStatus::Failed,
            _ => AlbumWriteStatus::Partial,
        }
    }
}
