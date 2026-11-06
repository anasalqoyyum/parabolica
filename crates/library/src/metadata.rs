//! Metadata reads never decode audio. FLAC pictures are skipped until requested.

use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::Path;

use parabolica_domain::control::CancelToken;
use parabolica_domain::ids::LibraryRootId;
use parabolica_storage::collection::SongMetadata;
use parabolica_storage::library::DisplayTag;
use symphonia::core::codecs::CodecParameters;
use symphonia::core::common::Limit;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, TrackType};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::{MetadataOptions, RawValue, StandardTag};

pub const MAX_TAG_BYTES: usize = 1024 * 1024;
pub const MAX_ART_BYTES: usize = 8 * 1024 * 1024;
const MAX_CONTAINER_HEADERS: usize = 4096;

#[derive(Debug, Clone)]
pub struct Metadata {
    pub tags: Vec<DisplayTag>,
    pub duration_ms: u64,
    pub format: String,
}

pub fn supported(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| {
        ["flac", "wav", "mp3", "m4a", "mp4", "aac", "ogg", "opus"]
            .contains(&e.to_ascii_lowercase().as_str())
    })
}

pub fn read(path: &Path) -> io::Result<Metadata> {
    read_with_cancel(path, &CancelToken::new())
}

pub fn read_with_cancel(path: &Path, cancel: &CancelToken) -> io::Result<Metadata> {
    check_cancel(cancel)?;
    let mut file = File::open(path)?;
    let mut magic = [0; 4];
    file.read_exact(&mut magic)?;
    if &magic == b"fLaC" {
        return read_flac(file, cancel);
    }
    let mut reader = probe_file(path, file, magic, cancel)?;
    let track = reader
        .default_track(TrackType::Audio)
        .ok_or_else(|| invalid("No audio track"))?;
    let Some(CodecParameters::Audio(audio)) = &track.codec_params else {
        return Err(invalid("No audio metadata"));
    };
    let duration_ms = track
        .num_frames
        .zip(audio.sample_rate)
        .map_or(0, |(n, r)| n.saturating_mul(1000) / u64::from(r.max(1)));
    let format = format!(
        "{} · {} Hz",
        reader.format_info().short_name.to_uppercase(),
        audio.sample_rate.unwrap_or(0)
    );
    let mut tags = Vec::new();
    if let Some(rev) = reader.metadata().skip_to_latest() {
        for tag in rev
            .media
            .tags
            .iter()
            .chain(rev.per_track.iter().flat_map(|t| &t.metadata.tags))
        {
            let field = tag.raw.key.trim().to_ascii_uppercase();
            if !field.is_empty() {
                let field = if field == "TXXX" {
                    tag.raw
                        .sub_fields
                        .as_ref()
                        .and_then(|fields| {
                            fields
                                .iter()
                                .find(|f| f.field.eq_ignore_ascii_case("description"))
                        })
                        .map(|f| f.value.to_string())
                        .filter(|value| !value.trim().is_empty())
                        .map(|v| v.trim().to_ascii_uppercase())
                        .unwrap_or(field)
                } else {
                    field
                };
                append_raw_value(&mut tags, &field, &tag.raw.value)?;
                if let Some(fields) = &tag.raw.sub_fields {
                    for sub in fields {
                        append_raw_value(
                            &mut tags,
                            &format!("{}.{}", tag.raw.key, sub.field).to_ascii_uppercase(),
                            &sub.value,
                        )?;
                    }
                }
            }
            let mapped = match &tag.std {
                Some(StandardTag::TrackTitle(v)) => Some(("TITLE", v.to_string())),
                Some(StandardTag::Album(v)) => Some(("ALBUM", v.to_string())),
                Some(StandardTag::AlbumArtist(v)) => Some(("ALBUMARTIST", v.to_string())),
                Some(StandardTag::Artist(v)) => Some(("ARTIST", v.to_string())),
                Some(StandardTag::TrackNumber(v)) => Some(("TRACKNUMBER", v.to_string())),
                Some(StandardTag::DiscNumber(v)) => Some(("DISCNUMBER", v.to_string())),
                Some(StandardTag::CompilationFlag(v)) => {
                    Some(("COMPILATION", u8::from(*v).to_string()))
                }
                Some(StandardTag::MusicBrainzAlbumId(v)) => {
                    Some(("MUSICBRAINZ_ALBUMID", v.to_string()))
                }
                Some(StandardTag::EditionTitle(v)) => Some(("EDITION", v.to_string())),
                Some(StandardTag::IdentCatalogNumber(v)) => Some(("CATALOGNUMBER", v.to_string())),
                Some(StandardTag::Genre(v)) => Some(("GENRE", v.to_string())),
                Some(StandardTag::Composer(v)) => Some(("COMPOSER", v.to_string())),
                Some(StandardTag::Label(v)) => Some(("LABEL", v.to_string())),
                Some(StandardTag::ReleaseDate(v)) => Some(("DATE", v.to_string())),
                _ => None,
            };
            if let Some((field, value)) = mapped
                && !tags
                    .iter()
                    .any(|tag| tag.field == field && tag.value == value)
            {
                tags.push(DisplayTag::new(field, value));
            }
        }
    }
    if tags
        .iter()
        .map(|tag| tag.field.len() + tag.value.len())
        .sum::<usize>()
        > MAX_ART_BYTES
    {
        return Err(invalid("Decoded metadata exceeds 8 MiB"));
    }
    Ok(Metadata {
        tags,
        duration_ms,
        format,
    })
}

fn append_raw_value(tags: &mut Vec<DisplayTag>, field: &str, value: &RawValue) -> io::Result<()> {
    if let RawValue::StringList(values) = value {
        for value in values.iter() {
            append_raw_value(tags, field, &RawValue::from(value.clone()))?;
        }
    } else {
        if matches!(value,RawValue::Binary(bytes) if bytes.len()>MAX_TAG_BYTES/5) {
            return Err(invalid("Binary metadata exceeds the tag limit"));
        }
        let value = value.to_string();
        if value.len() > MAX_TAG_BYTES {
            return Err(invalid("Decoded tag exceeds 1 MiB"));
        }
        let tag = DisplayTag::new(field, value);
        if tags.len() >= 16384 {
            return Err(invalid("Too many decoded tag values"));
        }
        tags.push(tag);
    }
    Ok(())
}

fn probe_file(
    path: &Path,
    mut file: File,
    magic: [u8; 4],
    cancel: &CancelToken,
) -> io::Result<Box<dyn symphonia::core::formats::FormatReader>> {
    preflight(&mut file, magic, cancel)?;
    file.rewind()?;
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }
    let options = MetadataOptions::default()
        .limit_tag_bytes(Limit::Maximum(MAX_TAG_BYTES))
        .limit_visual_bytes(Limit::Maximum(MAX_ART_BYTES));
    let reader = symphonia::default::get_probe()
        .probe(
            &hint,
            MediaSourceStream::new(
                Box::new(CancellableFile {
                    file,
                    cancel: cancel.clone(),
                }),
                Default::default(),
            ),
            FormatOptions::default(),
            options,
        )
        .map_err(io::Error::other)?;
    Ok(reader)
}

pub(crate) fn embedded_picture(path: &Path, cancel: &CancelToken) -> io::Result<Option<Vec<u8>>> {
    let mut file = File::open(path)?;
    let mut magic = [0; 4];
    file.read_exact(&mut magic)?;
    let mut reader = probe_file(path, file, magic, cancel)?;
    let mut metadata = reader.metadata();
    let Some(revision) = metadata.skip_to_latest() else {
        return Ok(None);
    };
    let visuals = revision.media.visuals.iter().chain(
        revision
            .per_track
            .iter()
            .flat_map(|track| &track.metadata.visuals),
    );
    Ok(visuals
        .filter(|v| v.data.len() <= MAX_ART_BYTES)
        .min_by_key(|v| v.usage != Some(symphonia::core::meta::StandardVisualKey::FrontCover))
        .map(|v| v.data.to_vec()))
}

// Symphonia's limits are advisory. Reject oversized metadata containers before probing.
fn preflight(file: &mut File, magic: [u8; 4], cancel: &CancelToken) -> io::Result<()> {
    let len = file.metadata()?.len();
    if &magic[..3] == b"ID3" {
        file.seek(SeekFrom::Start(6))?;
        let mut size = [0; 4];
        file.read_exact(&mut size)?;
        if size.iter().any(|b| b & 0x80 != 0) {
            return Err(invalid("Invalid ID3 size"));
        }
        let size = size
            .into_iter()
            .fold(0usize, |a, b| (a << 7) | usize::from(b));
        if size > MAX_TAG_BYTES {
            return Err(invalid("ID3 metadata exceeds 1 MiB"));
        }
    } else if &magic == b"RIFF" {
        file.seek(SeekFrom::Start(12))?;
        let mut headers = 0;
        let mut metadata_bytes = 0u64;
        while file.stream_position()?.saturating_add(8) <= len {
            check_cancel(cancel)?;
            let mut head = [0; 8];
            file.read_exact(&mut head)?;
            let size = u32::from_le_bytes(head[4..].try_into().unwrap()) as u64;
            headers += 1;
            if headers > MAX_CONTAINER_HEADERS {
                return Err(invalid("WAV metadata exceeds 4096 chunks"));
            }
            if &head[..4] != b"data" {
                metadata_bytes = metadata_bytes.saturating_add(size);
            }
            if metadata_bytes > MAX_ART_BYTES as u64 {
                return Err(invalid("WAV metadata exceeds 8 MiB in total"));
            }
            if &head[..4] != b"data" && size > MAX_TAG_BYTES as u64 {
                return Err(invalid("WAV metadata exceeds 1 MiB"));
            }
            let end = file
                .stream_position()?
                .checked_add(size + size % 2)
                .ok_or_else(|| invalid("Invalid WAV chunk"))?;
            if end > len {
                return Err(invalid("Truncated WAV chunk"));
            }
            file.seek(SeekFrom::Start(end))?;
        }
    } else if &magic == b"OggS" {
        file.rewind()?;
        let mut packet_bytes = 0usize;
        let mut packets = 0;
        for _ in 0..256 {
            check_cancel(cancel)?;
            let mut header = [0; 27];
            file.read_exact(&mut header)?;
            if &header[..4] != b"OggS" {
                return Err(invalid("Invalid Ogg page"));
            }
            let mut lacing = vec![0; usize::from(header[26])];
            file.read_exact(&mut lacing)?;
            for size in &lacing {
                packet_bytes += usize::from(*size);
                if packet_bytes > MAX_TAG_BYTES {
                    return Err(invalid("Ogg metadata packet exceeds 1 MiB"));
                }
                if *size < 255 {
                    packets += 1;
                    packet_bytes = 0;
                }
            }
            file.seek(SeekFrom::Current(
                lacing.iter().map(|v| i64::from(*v)).sum(),
            ))?;
            if packets >= 3 || header[5] & 4 != 0 {
                return Ok(());
            }
        }
        return Err(invalid("Ogg metadata exceeds 256 pages"));
    } else {
        file.rewind()?;
        let mut first = true;
        let mut headers = 0;
        let mut metadata_bytes = 0u64;
        while file.stream_position()?.saturating_add(8) <= len {
            check_cancel(cancel)?;
            let mut head = [0; 8];
            file.read_exact(&mut head)?;
            if first && &head[4..] != b"ftyp" {
                break;
            }
            first = false;
            headers += 1;
            if headers > MAX_CONTAINER_HEADERS {
                return Err(invalid("MP4 metadata exceeds 4096 atoms"));
            }
            let mut size = u32::from_be_bytes(head[..4].try_into().unwrap()) as u64;
            let header = if size == 1 {
                let mut ext = [0; 8];
                file.read_exact(&mut ext)?;
                size = u64::from_be_bytes(ext);
                16
            } else {
                8
            };
            if size == 0 {
                size = len - file.stream_position()? + header;
            }
            if size < header {
                return Err(invalid("Invalid MP4 atom"));
            }
            if &head[4..] != b"mdat" {
                metadata_bytes = metadata_bytes.saturating_add(size);
            }
            if metadata_bytes > MAX_ART_BYTES as u64 {
                return Err(invalid("MP4 metadata exceeds 8 MiB in total"));
            }
            let end = file
                .stream_position()?
                .checked_add(size - header)
                .ok_or_else(|| invalid("Invalid MP4 size"))?;
            if end > len {
                return Err(invalid("Truncated MP4 atom"));
            }
            file.seek(SeekFrom::Start(end))?;
        }
    }
    Ok(())
}

fn read_flac(mut file: File, cancel: &CancelToken) -> io::Result<Metadata> {
    let len = file.metadata()?.len();
    let mut tags = Vec::new();
    let mut duration_ms = 0;
    let mut format = None;
    for block in 0..65536 {
        check_cancel(cancel)?;
        let (kind, last, size) = flac_header(&mut file)?;
        let end = file
            .stream_position()?
            .checked_add(size as u64)
            .filter(|end| *end <= len)
            .ok_or_else(|| invalid("Truncated FLAC metadata"))?;
        match kind {
            0 if block == 0 && size == 34 => {
                let mut info = [0; 34];
                file.read_exact(&mut info)?;
                let packed = u64::from_be_bytes(info[10..18].try_into().unwrap());
                let rate = (packed >> 44) as u32;
                let bits = ((packed >> 36) & 31) + 1;
                let frames = packed & 0xfffffffff;
                if rate == 0 {
                    return Err(invalid("Invalid FLAC sample rate"));
                }
                duration_ms = frames.saturating_mul(1000) / u64::from(rate);
                format = Some(format!("FLAC · {bits}-bit · {rate} Hz"));
            }
            _ if block == 0 => return Err(invalid("Missing FLAC stream information")),
            4 => {
                if size > MAX_TAG_BYTES {
                    return Err(invalid("FLAC tags exceed 1 MiB"));
                }
                let mut data = vec![0; size];
                file.read_exact(&mut data)?;
                tags = vorbis_comments(&data)?;
            }
            _ => {}
        }
        file.seek(SeekFrom::Start(end))?;
        if last {
            return Ok(Metadata {
                tags,
                duration_ms,
                format: format.ok_or_else(|| invalid("Missing FLAC format"))?,
            });
        }
    }
    Err(invalid("Too many FLAC metadata blocks"))
}

pub(crate) fn flac_header(file: &mut File) -> io::Result<(u8, bool, usize)> {
    let mut h = [0; 4];
    file.read_exact(&mut h)?;
    Ok((
        h[0] & 127,
        h[0] & 128 != 0,
        usize::from(h[1]) << 16 | usize::from(h[2]) << 8 | usize::from(h[3]),
    ))
}
fn vorbis_comments(mut data: &[u8]) -> io::Result<Vec<DisplayTag>> {
    fn number(data: &mut &[u8]) -> io::Result<usize> {
        let bytes = data
            .get(..4)
            .ok_or_else(|| invalid("Truncated FLAC comment"))?;
        let n = u32::from_le_bytes(bytes.try_into().unwrap()) as usize;
        *data = &data[4..];
        Ok(n)
    }
    let vendor = number(&mut data)?;
    data = data
        .get(vendor..)
        .ok_or_else(|| invalid("Invalid FLAC vendor"))?;
    let count = number(&mut data)?;
    if count > 16384 {
        return Err(invalid("Too many FLAC tags"));
    }
    let mut tags = Vec::new();
    for _ in 0..count {
        let len = number(&mut data)?;
        let value = std::str::from_utf8(
            data.get(..len)
                .ok_or_else(|| invalid("Invalid FLAC tag length"))?,
        )
        .map_err(io::Error::other)?;
        if let Some((field, value)) = value.split_once('=')
            && !field.trim().is_empty()
        {
            tags.push(DisplayTag::new(field.to_ascii_uppercase(), value));
        }
        data = &data[len..];
    }
    Ok(tags)
}
fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

impl Metadata {
    pub fn projection(
        &self,
        root: LibraryRootId,
        relative: &Path,
        error: Option<String>,
    ) -> SongMetadata {
        let first = |key: &str| {
            self.tags
                .iter()
                .find(|t| t.field == key && !t.value.trim().is_empty())
                .map(|t| t.value.trim().to_owned())
        };
        let number = |key: &str| {
            first(key)
                .and_then(|v| v.split('/').next()?.trim().parse::<u32>().ok())
                .unwrap_or(0)
        };
        let folder = relative.parent().unwrap_or(Path::new(""));
        let mut album_folder = folder;
        if number("DISCNUMBER") > 0
            && folder
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(is_disc_folder)
        {
            album_folder = folder.parent().unwrap_or(Path::new(""));
        }
        let album = first("ALBUM").unwrap_or_else(|| {
            album_folder
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned()
        });
        let album = if album.is_empty() {
            "Unknown album".into()
        } else {
            album
        };
        let mut artists = self
            .tags
            .iter()
            .filter(|t| t.field == "ARTIST" && !t.value.trim().is_empty())
            .map(|t| t.value.trim().to_owned())
            .collect::<Vec<_>>();
        if artists.is_empty() {
            artists.push("Unknown artist".into());
        }
        let compilation =
            first("COMPILATION").is_some_and(|v| v == "1" || v.eq_ignore_ascii_case("true"));
        let album_artist = first("ALBUMARTIST")
            .or_else(|| first("ALBUM ARTIST"))
            .unwrap_or_else(|| {
                if compilation {
                    "Various artists".into()
                } else {
                    artists[0].clone()
                }
            });
        let edition_values = ["MUSICBRAINZ_ALBUMID", "CATALOGNUMBER", "EDITION", "DATE"]
            .map(|k| first(k).unwrap_or_default());
        let edition = edition_values
            .iter()
            .filter(|v| !v.is_empty())
            .cloned()
            .collect::<Vec<_>>()
            .join(" · ");
        // Length-prefix each part so embedded separators in tags cannot collide.
        let parts = [
            root.to_string(),
            path_key(album_folder).to_string_lossy().into_owned(),
            album_artist.clone(),
            album.clone(),
        ];
        let group_key = parts
            .iter()
            .chain(edition_values.iter())
            .map(|v| format!("{}:{v}", v.len()))
            .collect::<String>();
        SongMetadata {
            group_key,
            album,
            album_artist,
            edition,
            title: first("TITLE").unwrap_or_else(|| {
                relative
                    .file_stem()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned()
            }),
            artists,
            folder: folder.to_owned(),
            disc: number("DISCNUMBER").max(1),
            track: number("TRACKNUMBER"),
            duration_ms: self.duration_ms,
            format: self.format.clone(),
            error,
        }
    }
}
fn is_disc_folder(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    ["cd", "disc", "disk"].iter().any(|prefix| {
        lower.strip_prefix(prefix).is_some_and(|n| {
            let n = n.trim_matches([' ', '-', '_']);
            !n.is_empty() && n.chars().all(|c| c.is_ascii_digit())
        })
    })
}

pub(crate) fn path_key(path: &Path) -> std::path::PathBuf {
    #[cfg(windows)]
    {
        std::path::PathBuf::from(path.to_string_lossy().to_lowercase())
    }
    #[cfg(not(windows))]
    {
        path.to_owned()
    }
}

fn check_cancel(cancel: &CancelToken) -> io::Result<()> {
    if cancel.is_cancelled() {
        Err(io::Error::new(
            io::ErrorKind::Interrupted,
            "Indexing cancelled",
        ))
    } else {
        Ok(())
    }
}
#[derive(Debug)]
struct CancellableFile {
    file: File,
    cancel: CancelToken,
}
impl Read for CancellableFile {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        check_cancel(&self.cancel)?;
        self.file.read(buf)
    }
}
impl Seek for CancellableFile {
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        check_cancel(&self.cancel)?;
        self.file.seek(position)
    }
}
impl symphonia::core::io::MediaSource for CancellableFile {
    fn is_seekable(&self) -> bool {
        true
    }
    fn byte_len(&self) -> Option<u64> {
        self.file.metadata().ok().map(|m| m.len())
    }
}
