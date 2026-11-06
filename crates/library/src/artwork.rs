//! Lazy artwork decoding with input, pixel, thumbnail and cache limits.

use std::collections::VecDeque;
use std::fs::File;
use std::io::{self, Cursor, Read, Seek, SeekFrom};
use std::path::Path;
use std::sync::Arc;

use image::{ImageFormat, ImageReader, Limits};
use parabolica_domain::control::CancelToken;
use parabolica_domain::files::FileSignature;
use parabolica_domain::ids::MediaFileId;

use crate::metadata::{MAX_ART_BYTES, flac_header};

pub const THUMBNAIL_EDGE: u32 = 256;
const CACHE_ENTRIES: usize = 64;
const MAX_PIXELS: u64 = 16_000_000;

type CacheEntry = (MediaFileId, FileSignature, Option<Arc<Vec<u8>>>);

#[derive(Debug, Default)]
pub struct ThumbnailCache {
    entries: VecDeque<CacheEntry>,
}
impl ThumbnailCache {
    pub fn load(
        &mut self,
        id: MediaFileId,
        signature: FileSignature,
        path: &Path,
        cancel: &CancelToken,
    ) -> io::Result<Option<Arc<Vec<u8>>>> {
        if let Some(index) = self
            .entries
            .iter()
            .position(|(key, sig, _)| *key == id && *sig == signature)
        {
            let entry = self.entries.remove(index).unwrap();
            let bytes = entry.2.clone();
            self.entries.push_back(entry);
            return Ok(bytes);
        }
        if cancel.is_cancelled() {
            return Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "Artwork cancelled",
            ));
        }
        let bytes = read_picture(path, cancel)?
            .map(|data| thumbnail(&data, cancel))
            .transpose()?
            .map(Arc::new);
        self.entries.retain(|(key, _, _)| *key != id);
        while self.entries.len() >= CACHE_ENTRIES {
            self.entries.pop_front();
        }
        self.entries.push_back((id, signature, bytes.clone()));
        Ok(bytes)
    }
}

pub fn thumbnail(data: &[u8], cancel: &CancelToken) -> io::Result<Vec<u8>> {
    if data.len() > MAX_ART_BYTES {
        return Err(invalid("Artwork exceeds 8 MiB"));
    }
    let reader = ImageReader::new(Cursor::new(data)).with_guessed_format()?;
    let (width, height) = reader.into_dimensions().map_err(io::Error::other)?;
    if u64::from(width) * u64::from(height) > MAX_PIXELS {
        return Err(invalid("Artwork exceeds 16 million pixels"));
    }
    let mut reader = ImageReader::new(Cursor::new(data)).with_guessed_format()?;
    let mut limits = Limits::default();
    limits.max_image_width = Some(8192);
    limits.max_image_height = Some(8192);
    limits.max_alloc = Some(128 * 1024 * 1024);
    reader.limits(limits);
    if cancel.is_cancelled() {
        return Err(io::Error::new(
            io::ErrorKind::Interrupted,
            "Artwork cancelled",
        ));
    }
    let decoded = reader.decode().map_err(io::Error::other)?;
    if cancel.is_cancelled() {
        return Err(io::Error::new(
            io::ErrorKind::Interrupted,
            "Artwork cancelled",
        ));
    }
    let mut out = Cursor::new(Vec::new());
    decoded
        .thumbnail(THUMBNAIL_EDGE, THUMBNAIL_EDGE)
        .write_to(&mut out, ImageFormat::Png)
        .map_err(io::Error::other)?;
    Ok(out.into_inner())
}

fn read_picture(path: &Path, cancel: &CancelToken) -> io::Result<Option<Vec<u8>>> {
    let mut file = File::open(path)?;
    let mut magic = [0; 4];
    file.read_exact(&mut magic)?;
    if &magic == b"fLaC" {
        let file_len = file.metadata()?.len();
        for _ in 0..65536 {
            if cancel.is_cancelled() {
                return Err(io::Error::new(
                    io::ErrorKind::Interrupted,
                    "Artwork cancelled",
                ));
            }
            let (kind, last, size) = flac_header(&mut file)?;
            let end = file
                .stream_position()?
                .checked_add(size as u64)
                .filter(|end| *end <= file_len)
                .ok_or_else(|| invalid("Truncated artwork block"))?;
            if kind == 6 && size <= MAX_ART_BYTES {
                let mut block = vec![0; size];
                file.read_exact(&mut block)?;
                if let Some(bytes) = flac_picture(&block) {
                    return Ok(Some(bytes.to_vec()));
                }
            }
            file.seek(SeekFrom::Start(end))?;
            if last {
                break;
            }
        }
    }
    if &magic != b"fLaC"
        && let Ok(Some(bytes)) = crate::metadata::embedded_picture(path, cancel)
    {
        return Ok(Some(bytes));
    }
    for name in ["cover.jpg", "cover.png", "folder.jpg", "folder.png"] {
        let candidate = path.with_file_name(name);
        match File::open(candidate) {
            Ok(mut file) => {
                let mut data = Vec::new();
                file.by_ref()
                    .take(MAX_ART_BYTES as u64 + 1)
                    .read_to_end(&mut data)?;
                if data.len() <= MAX_ART_BYTES {
                    return Ok(Some(data));
                }
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
    }
    Ok(None)
}
fn flac_picture(mut bytes: &[u8]) -> Option<&[u8]> {
    fn n(bytes: &mut &[u8]) -> Option<usize> {
        let n = u32::from_be_bytes(bytes.get(..4)?.try_into().ok()?) as usize;
        *bytes = &bytes[4..];
        Some(n)
    }
    let kind = n(&mut bytes)?;
    if kind != 3 && kind != 0 {
        return None;
    }
    let mime = n(&mut bytes)?;
    bytes = bytes.get(mime..)?;
    let description = n(&mut bytes)?;
    bytes = bytes.get(description..)?;
    bytes = bytes.get(16..)?;
    let len = n(&mut bytes)?;
    bytes.get(..len)
}
fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}
