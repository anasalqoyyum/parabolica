//! FLAC metadata blocks, Vorbis comments and pictures.
//!
//! Blocks are kept as raw bytes. A write re-encodes only the Vorbis comment block and the picture
//! blocks it replaces; every other block and the audio frames are copied byte for byte.

use std::io::Read;

use parabolica_domain::tags::PictureType;

use crate::TagError;

pub(crate) const STREAMINFO: u8 = 0;
pub(crate) const VORBIS_COMMENT: u8 = 4;
pub(crate) const PICTURE: u8 = 6;
/// The largest block a 24-bit length field can describe.
pub(crate) const MAX_BLOCK_BYTES: usize = (1 << 24) - 1;
const MAX_METADATA_BYTES: u64 = 64 * 1024 * 1024;
const MAX_BLOCKS: usize = 4096;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Block {
    pub kind: u8,
    pub data: Vec<u8>,
}

/// The metadata blocks in file order and the byte offset where audio frames begin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Metadata {
    pub blocks: Vec<Block>,
    pub audio_offset: u64,
}

pub(crate) fn read_metadata(reader: &mut impl Read) -> Result<Metadata, TagError> {
    let mut magic = [0; 4];
    reader
        .read_exact(&mut magic)
        .map_err(|_| TagError::NotFlac)?;
    if &magic != b"fLaC" {
        return Err(TagError::NotFlac);
    }
    let mut blocks = Vec::new();
    let mut offset = 4u64;
    loop {
        let mut header = [0; 4];
        reader
            .read_exact(&mut header)
            .map_err(|_| TagError::Malformed("metadata ends early"))?;
        let last = header[0] & 0x80 != 0;
        let kind = header[0] & 0x7f;
        if kind == 127 {
            return Err(TagError::Malformed("invalid metadata block type"));
        }
        let length = u32::from_be_bytes([0, header[1], header[2], header[3]]);
        offset += 4 + u64::from(length);
        if offset > MAX_METADATA_BYTES {
            return Err(TagError::TooLarge("FLAC metadata exceeds 64 MiB"));
        }
        let mut data = vec![0; length as usize];
        reader
            .read_exact(&mut data)
            .map_err(|_| TagError::Malformed("metadata block is truncated"))?;
        if blocks.is_empty() && kind != STREAMINFO {
            return Err(TagError::Malformed("STREAMINFO is not the first block"));
        }
        blocks.push(Block { kind, data });
        if blocks.len() > MAX_BLOCKS {
            return Err(TagError::TooLarge("FLAC file has too many metadata blocks"));
        }
        if last {
            break;
        }
    }
    Ok(Metadata {
        blocks,
        audio_offset: offset,
    })
}

/// `fLaC` followed by the blocks, with the last-block flag on the final one.
pub(crate) fn encode_metadata(blocks: &[Block]) -> Result<Vec<u8>, TagError> {
    let mut out = b"fLaC".to_vec();
    for (index, block) in blocks.iter().enumerate() {
        if block.data.len() > MAX_BLOCK_BYTES {
            return Err(TagError::TooLarge("a metadata block exceeds 16 MiB"));
        }
        let last = if index + 1 == blocks.len() { 0x80 } else { 0 };
        out.push(last | block.kind);
        out.extend_from_slice(&(block.data.len() as u32).to_be_bytes()[1..]);
        out.extend_from_slice(&block.data);
    }
    Ok(out)
}

/// Comments keep their raw bytes so entries the edit does not touch are written back unchanged,
/// including any that are not valid UTF-8.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct VorbisComments {
    pub vendor: Vec<u8>,
    pub entries: Vec<Vec<u8>>,
}

impl VorbisComments {
    pub fn decode(data: &[u8]) -> Result<Self, TagError> {
        let mut cursor = Cursor(data);
        let vendor = cursor.le_bytes()?.to_vec();
        let count = cursor.le_u32()?;
        let mut entries = Vec::new();
        for _ in 0..count {
            entries.push(cursor.le_bytes()?.to_vec());
        }
        Ok(Self { vendor, entries })
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&(self.vendor.len() as u32).to_le_bytes());
        out.extend_from_slice(&self.vendor);
        out.extend_from_slice(&(self.entries.len() as u32).to_le_bytes());
        for entry in &self.entries {
            out.extend_from_slice(&(entry.len() as u32).to_le_bytes());
            out.extend_from_slice(entry);
        }
        out
    }

    /// Entries whose key is one of `keys` (compared ignoring ASCII case).
    pub fn has_key(entry: &[u8], keys: &[String]) -> bool {
        let key = entry.split(|b| *b == b'=').next().unwrap_or_default();
        keys.iter().any(|k| k.as_bytes().eq_ignore_ascii_case(key))
    }

    pub fn value(entry: &[u8]) -> Option<String> {
        let at = entry.iter().position(|b| *b == b'=')?;
        Some(String::from_utf8_lossy(&entry[at + 1..]).into_owned())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Picture {
    pub code: u32,
    pub mime: String,
    pub description: String,
    pub width: u32,
    pub height: u32,
    pub depth: u32,
    pub colors: u32,
    pub data: Vec<u8>,
}

impl Picture {
    pub fn decode(data: &[u8]) -> Result<Self, TagError> {
        let mut cursor = Cursor(data);
        let code = cursor.be_u32()?;
        let mime = String::from_utf8_lossy(cursor.be_bytes()?).into_owned();
        let description = String::from_utf8_lossy(cursor.be_bytes()?).into_owned();
        Ok(Self {
            code,
            mime,
            description,
            width: cursor.be_u32()?,
            height: cursor.be_u32()?,
            depth: cursor.be_u32()?,
            colors: cursor.be_u32()?,
            data: cursor.be_bytes()?.to_vec(),
        })
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(32 + self.mime.len() + self.data.len());
        out.extend_from_slice(&self.code.to_be_bytes());
        for bytes in [self.mime.as_bytes(), self.description.as_bytes()] {
            out.extend_from_slice(&(bytes.len() as u32).to_be_bytes());
            out.extend_from_slice(bytes);
        }
        for value in [self.width, self.height, self.depth, self.colors] {
            out.extend_from_slice(&value.to_be_bytes());
        }
        out.extend_from_slice(&(self.data.len() as u32).to_be_bytes());
        out.extend_from_slice(&self.data);
        out
    }
}

/// FLAC/ID3 picture type codes. Only code 0 counts as [`PictureType::Other`]; icons, artist
/// photos and the other standard types are never replaced by an edit.
pub(crate) fn picture_code(kind: PictureType) -> u32 {
    match kind {
        PictureType::Other => 0,
        PictureType::FrontCover => 3,
        PictureType::BackCover => 4,
        PictureType::Booklet => 5,
    }
}

pub(crate) fn picture_type(code: u32) -> Option<PictureType> {
    [
        PictureType::Other,
        PictureType::FrontCover,
        PictureType::BackCover,
        PictureType::Booklet,
    ]
    .into_iter()
    .find(|kind| picture_code(*kind) == code)
}

struct Cursor<'a>(&'a [u8]);

impl<'a> Cursor<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], TagError> {
        if self.0.len() < n {
            return Err(TagError::Malformed("metadata field is truncated"));
        }
        let (head, rest) = self.0.split_at(n);
        self.0 = rest;
        Ok(head)
    }

    fn le_u32(&mut self) -> Result<u32, TagError> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }

    fn be_u32(&mut self) -> Result<u32, TagError> {
        Ok(u32::from_be_bytes(self.take(4)?.try_into().unwrap()))
    }

    fn le_bytes(&mut self) -> Result<&'a [u8], TagError> {
        let n = self.le_u32()? as usize;
        self.take(n)
    }

    fn be_bytes(&mut self) -> Result<&'a [u8], TagError> {
        let n = self.be_u32()? as usize;
        self.take(n)
    }
}
