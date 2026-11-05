//! Physical file identity and change detection.

use std::path::PathBuf;

use crate::ids::LibraryRootId;

/// Opaque volume identity reported by the platform layer (for example a Windows volume GUID path).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct VolumeId(pub String);

/// OS-level file identity, such as the NTFS 128-bit file ID, when the volume provides one.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct OsFileId {
    pub volume: VolumeId,
    pub file_id: u128,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ContentDigest(pub [u8; 32]);

/// The facts recorded when a file was indexed, analysed or reviewed for a tag write.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileSignature {
    pub size_bytes: u64,
    pub modified_unix_nanos: i128,
    pub content_digest: Option<ContentDigest>,
}

impl FileSignature {
    /// Whether the file still matches a recorded signature.
    ///
    /// Size and modification time must match. When both sides carry a digest it must match too,
    /// so a rewrite that preserved size and mtime is still detected.
    pub fn matches_recorded(&self, recorded: &FileSignature) -> bool {
        if self.size_bytes != recorded.size_bytes
            || self.modified_unix_nanos != recorded.modified_unix_nanos
        {
            return false;
        }
        match (&self.content_digest, &recorded.content_digest) {
            (Some(current), Some(previous)) => current == previous,
            _ => true,
        }
    }
}

/// Where a physical file lives. The path is relative to its library root so a root can be
/// relocated without reassigning file identities.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FileLocation {
    pub root: LibraryRootId,
    pub relative_path: PathBuf,
}

/// Root availability. An unplugged root keeps its files; they are not deleted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RootAvailability {
    Online,
    Offline,
    PermissionDenied,
}
