//! Tag and artwork reads and safe album writes.
//!
//! [`write`] replaces one file through a verified temporary sibling. [`job`] runs a reviewed album
//! write file by file and reports each outcome to a [`job::Ledger`] before and after the
//! replacement, so an interrupted job can be recovered. ReplayGain and Discogs consumers build
//! [`parabolica_domain::tags::TagPatch`]es and use the same path.
//!
//! FLAC (Vorbis comments and PICTURE blocks) is the only writable format. Other formats are
//! reported as unsupported before review.

pub mod artwork;
pub mod discogs_import;
pub mod fields;
mod flac;
pub mod job;
pub mod write;

use std::io;
use std::path::Path;

use parabolica_domain::tags::{PictureType, TagField};

#[derive(Debug, thiserror::Error)]
pub enum TagError {
    #[error("Not a FLAC file")]
    NotFlac,
    #[error("Malformed FLAC metadata: {0}")]
    Malformed(&'static str),
    #[error("{0}")]
    TooLarge(&'static str),
    #[error("Invalid field name {0:?}")]
    InvalidField(String),
    #[error("{0} contains a NUL character")]
    InvalidValue(String),
    #[error("{0}")]
    Io(#[from] io::Error),
}

/// Whether this build can write tags to the file, judged by its extension. The content is
/// checked again when the file is read for review.
pub fn writable_format(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("flac"))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PictureInfo {
    /// The FLAC/ID3 picture type code.
    pub code: u32,
    /// `None` for picture types an edit never replaces, such as file icons.
    pub kind: Option<PictureType>,
    pub mime: String,
    pub width: u32,
    pub height: u32,
    pub bytes: usize,
}

/// A file's comments and picture summaries as read for review.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileTags {
    comments: flac::VorbisComments,
    pub pictures: Vec<PictureInfo>,
}

impl FileTags {
    fn from_metadata(metadata: &flac::Metadata) -> Result<Self, TagError> {
        let mut comments = flac::VorbisComments::default();
        let mut pictures = Vec::new();
        for block in &metadata.blocks {
            match block.kind {
                flac::VORBIS_COMMENT => comments = flac::VorbisComments::decode(&block.data)?,
                flac::PICTURE => {
                    let picture = flac::Picture::decode(&block.data)?;
                    pictures.push(PictureInfo {
                        code: picture.code,
                        kind: flac::picture_type(picture.code),
                        mime: picture.mime,
                        width: picture.width,
                        height: picture.height,
                        bytes: picture.data.len(),
                    });
                }
                _ => {}
            }
        }
        Ok(Self { comments, pictures })
    }

    pub fn values(&self, field: &TagField) -> Vec<String> {
        fields::values(&self.comments, field).unwrap_or_default()
    }

    /// Every comment as `(key, value)`, in file order. Invalid UTF-8 is shown lossily.
    pub fn comments(&self) -> Vec<(String, String)> {
        self.comments
            .entries
            .iter()
            .map(|entry| {
                let text = String::from_utf8_lossy(entry);
                match text.split_once('=') {
                    Some((key, value)) => (key.to_owned(), value.to_owned()),
                    None => (text.into_owned(), String::new()),
                }
            })
            .collect()
    }

    pub fn has_picture(&self, kind: PictureType) -> bool {
        self.pictures.iter().any(|p| p.kind == Some(kind))
    }
}

#[cfg(test)]
mod tests;
