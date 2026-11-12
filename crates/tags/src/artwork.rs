//! Local artwork validation and sidecar names.

use std::io::{self, Cursor};
use std::path::Path;
use std::sync::Arc;

use image::{ImageFormat, ImageReader, Limits};
use parabolica_domain::tags::PictureType;

use crate::flac::{MAX_BLOCK_BYTES, Picture, picture_code};

/// Encoded image limit. A FLAC picture block also holds the MIME type and dimensions within its
/// 16 MiB length field.
pub const MAX_IMAGE_BYTES: usize = MAX_BLOCK_BYTES - 256;
pub const MAX_IMAGE_EDGE: u32 = 10_000;
const MAX_PIXELS: u64 = 40_000_000;
const MAX_DECODE_ALLOC: u64 = 256 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ArtworkError {
    #[error("Only JPEG and PNG images are supported")]
    UnsupportedFormat,
    #[error("Image is larger than 16 MiB")]
    TooLarge,
    #[error(
        "Image is {width} × {height}; the limit is {MAX_IMAGE_EDGE} px per side and 40 megapixels"
    )]
    TooManyPixels { width: u32, height: u32 },
    #[error("Image could not be decoded: {0}")]
    Decode(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageKind {
    Jpeg,
    Png,
}

impl ImageKind {
    pub fn mime(self) -> &'static str {
        match self {
            Self::Jpeg => "image/jpeg",
            Self::Png => "image/png",
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            Self::Jpeg => "jpg",
            Self::Png => "png",
        }
    }

    pub fn from_mime(mime: &str) -> Option<Self> {
        match mime {
            "image/jpeg" => Some(Self::Jpeg),
            "image/png" => Some(Self::Png),
            _ => None,
        }
    }
}

/// An image that decoded completely within the limits. Its bytes are written unchanged.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StagedImage {
    bytes: Arc<Vec<u8>>,
    kind: ImageKind,
    width: u32,
    height: u32,
    depth: u32,
}

impl StagedImage {
    pub fn decode(bytes: Vec<u8>) -> Result<Self, ArtworkError> {
        if bytes.len() > MAX_IMAGE_BYTES {
            return Err(ArtworkError::TooLarge);
        }
        let reader = ImageReader::new(Cursor::new(&bytes))
            .with_guessed_format()
            .map_err(|error| ArtworkError::Decode(error.to_string()))?;
        let kind = match reader.format() {
            Some(ImageFormat::Jpeg) => ImageKind::Jpeg,
            Some(ImageFormat::Png) => ImageKind::Png,
            _ => return Err(ArtworkError::UnsupportedFormat),
        };
        let (width, height) = reader
            .into_dimensions()
            .map_err(|error| ArtworkError::Decode(error.to_string()))?;
        if width == 0
            || height == 0
            || width > MAX_IMAGE_EDGE
            || height > MAX_IMAGE_EDGE
            || u64::from(width) * u64::from(height) > MAX_PIXELS
        {
            return Err(ArtworkError::TooManyPixels { width, height });
        }
        let mut reader = ImageReader::new(Cursor::new(&bytes))
            .with_guessed_format()
            .map_err(|error| ArtworkError::Decode(error.to_string()))?;
        let mut limits = Limits::default();
        limits.max_image_width = Some(MAX_IMAGE_EDGE);
        limits.max_image_height = Some(MAX_IMAGE_EDGE);
        limits.max_alloc = Some(MAX_DECODE_ALLOC);
        reader.limits(limits);
        let decoded = reader
            .decode()
            .map_err(|error| ArtworkError::Decode(error.to_string()))?;
        let depth = u32::from(decoded.color().bits_per_pixel());
        Ok(Self {
            bytes: Arc::new(bytes),
            kind,
            width,
            height,
            depth,
        })
    }

    pub fn read(path: &Path) -> Result<Self, ReadImageError> {
        let length = std::fs::metadata(path)?.len();
        if length > MAX_IMAGE_BYTES as u64 {
            return Err(ArtworkError::TooLarge.into());
        }
        Ok(Self::decode(std::fs::read(path)?)?)
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub fn kind(&self) -> ImageKind {
        self.kind
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub(crate) fn picture(&self, kind: PictureType) -> Picture {
        Picture {
            code: picture_code(kind),
            mime: self.kind.mime().to_owned(),
            description: String::new(),
            width: self.width,
            height: self.height,
            depth: self.depth,
            colors: 0,
            data: self.bytes.to_vec(),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ReadImageError {
    #[error("{0}")]
    Io(#[from] io::Error),
    #[error("{0}")]
    Artwork(#[from] ArtworkError),
}

const IMAGE_EXTENSIONS: [&str; 3] = ["jpg", "jpeg", "png"];

fn sidecar_stems(kind: PictureType) -> &'static [&'static str] {
    match kind {
        PictureType::FrontCover => &["cover", "folder", "front"],
        PictureType::BackCover => &["back"],
        PictureType::Booklet => &["booklet"],
        PictureType::Other => &[],
    }
}

/// The sidecar name for one image of a picture type: `cover.jpg`, `back.png`, `booklet.jpg`, or
/// `booklet-01.jpg` … when several booklet pages are written.
pub fn sidecar_name(
    kind: PictureType,
    index: usize,
    count: usize,
    image: ImageKind,
) -> Option<String> {
    let stem = sidecar_stems(kind).first()?;
    Some(if count > 1 {
        format!("{stem}-{:02}.{}", index + 1, image.extension())
    } else {
        format!("{stem}.{}", image.extension())
    })
}

/// Existing image files in `dir` that a sidecar of `kind` would replace, such as `folder.jpg` or
/// `Cover.PNG` for a front cover, sorted by name.
pub fn existing_sidecars(dir: &Path, kind: PictureType) -> io::Result<Vec<String>> {
    let stems = sidecar_stems(kind);
    let mut names = Vec::new();
    if stems.is_empty() {
        return Ok(names);
    }
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        let lower = name.to_ascii_lowercase();
        let Some((stem, extension)) = lower.rsplit_once('.') else {
            continue;
        };
        let stem_matches = stems.iter().any(|candidate| {
            stem == *candidate
                || (kind == PictureType::Booklet
                    && stem
                        .strip_prefix("booklet-")
                        .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit())))
        });
        if stem_matches && IMAGE_EXTENSIONS.contains(&extension) {
            names.push(name);
        }
    }
    names.sort();
    Ok(names)
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(crate) fn png(width: u32, height: u32) -> Vec<u8> {
        let mut out = Cursor::new(Vec::new());
        image::RgbImage::from_pixel(width, height, image::Rgb([200, 40, 10]))
            .write_to(&mut out, ImageFormat::Png)
            .unwrap();
        out.into_inner()
    }

    #[test]
    fn validates_format_size_and_content() {
        let image = StagedImage::decode(png(3, 2)).unwrap();
        assert_eq!((image.width(), image.height()), (3, 2));
        assert_eq!(image.kind(), ImageKind::Png);
        assert_eq!(
            StagedImage::decode(b"GIF89a....".to_vec()),
            Err(ArtworkError::UnsupportedFormat)
        );
        let mut truncated = png(64, 64);
        truncated.truncate(truncated.len() / 2);
        assert!(matches!(
            StagedImage::decode(truncated),
            Err(ArtworkError::Decode(_))
        ));
        assert_eq!(
            StagedImage::decode(vec![0; MAX_IMAGE_BYTES + 1]),
            Err(ArtworkError::TooLarge)
        );
    }

    #[test]
    fn sidecar_conflicts_match_names_case_insensitively() {
        let dir = tempfile::tempdir().unwrap();
        for name in [
            "Folder.JPG",
            "cover.png",
            "booklet-02.jpg",
            "notes.jpg",
            "back.txt",
        ] {
            std::fs::write(dir.path().join(name), b"x").unwrap();
        }
        assert_eq!(
            existing_sidecars(dir.path(), PictureType::FrontCover).unwrap(),
            vec!["Folder.JPG".to_owned(), "cover.png".to_owned()]
        );
        assert_eq!(
            existing_sidecars(dir.path(), PictureType::Booklet).unwrap(),
            vec!["booklet-02.jpg".to_owned()]
        );
        assert!(
            existing_sidecars(dir.path(), PictureType::BackCover)
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            sidecar_name(PictureType::Booklet, 1, 3, ImageKind::Jpeg).as_deref(),
            Some("booklet-02.jpg")
        );
        assert_eq!(
            sidecar_name(PictureType::FrontCover, 0, 1, ImageKind::Png).as_deref(),
            Some("cover.png")
        );
    }
}
