use std::fs::File;
use std::io::{self, Read as _, Seek as _, SeekFrom};
use std::path::Path;

#[derive(Debug, Clone, Copy)]
struct Atom {
    kind: [u8; 4],
    data: u64,
    end: u64,
}

pub(super) struct Presentation {
    pub delay: u32,
    pub frames: u64,
}

struct Metadata {
    file: File,
    atoms: usize,
}

impl Metadata {
    fn children(&mut self, parent: Atom) -> io::Result<Vec<Atom>> {
        let mut offset = parent.data;
        let mut children = Vec::new();
        while offset < parent.end {
            self.atoms += 1;
            if self.atoms > 4096 {
                return Err(invalid("too many MP4 metadata atoms"));
            }
            let header = self.bytes::<8>(parent, offset - parent.data)?;
            let size = u32::from_be_bytes(header[..4].try_into().unwrap());
            let (size, header_size) = match size {
                0 => (parent.end - offset, 8),
                1 => (
                    u64::from_be_bytes(self.bytes::<8>(parent, offset - parent.data + 8)?),
                    16,
                ),
                size => (u64::from(size), 8),
            };
            let end = offset
                .checked_add(size)
                .filter(|end| size >= header_size && *end <= parent.end)
                .ok_or_else(|| invalid("invalid MP4 metadata atom length"))?;
            children.push(Atom {
                kind: header[4..].try_into().unwrap(),
                data: offset + header_size,
                end,
            });
            offset = end;
        }
        Ok(children)
    }

    fn bytes<const N: usize>(&mut self, atom: Atom, offset: u64) -> io::Result<[u8; N]> {
        let start = atom
            .data
            .checked_add(offset)
            .filter(|start| {
                start
                    .checked_add(N as u64)
                    .is_some_and(|end| end <= atom.end)
            })
            .ok_or_else(|| invalid("truncated MP4 gapless metadata"))?;
        self.file.seek(SeekFrom::Start(start))?;
        let mut bytes = [0; N];
        self.file.read_exact(&mut bytes)?;
        Ok(bytes)
    }

    fn version(&mut self, atom: Atom) -> io::Result<u8> {
        let version = self.bytes::<1>(atom, 0)?[0];
        if version > 1 {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "MP4 timing metadata version",
            ));
        }
        Ok(version)
    }

    fn timing(&mut self, atom: Atom) -> io::Result<(u32, u64)> {
        let version = self.version(atom)?;
        let offset = if version == 0 { 12 } else { 20 };
        let scale = u32::from_be_bytes(self.bytes(atom, offset)?);
        if scale == 0 {
            return Err(invalid("zero MP4 timescale"));
        }
        let duration = if version == 0 {
            u64::from(u32::from_be_bytes(self.bytes(atom, offset + 4)?))
        } else {
            u64::from_be_bytes(self.bytes(atom, offset + 4)?)
        };
        Ok((scale, duration))
    }
}

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

fn find(atoms: &[Atom], kind: &[u8; 4]) -> Option<Atom> {
    atoms.iter().find(|atom| &atom.kind == kind).copied()
}

fn required(atoms: &[Atom], kind: &[u8; 4]) -> io::Result<Atom> {
    find(atoms, kind).ok_or_else(|| invalid("incomplete MP4 gapless metadata"))
}

fn frames(ticks: u64, scale: u32, rate: u32) -> io::Result<u64> {
    u64::try_from(u128::from(ticks) * u128::from(rate) / u128::from(scale))
        .map_err(|_| invalid("MP4 duration exceeds the supported range"))
}

pub(super) fn presentation(
    path: &Path,
    track: u32,
    rate: u32,
    media_frames: Option<u64>,
) -> io::Result<Option<Presentation>> {
    let file = File::open(path)?;
    let end = file.metadata()?.len();
    let mut metadata = Metadata { file, atoms: 0 };
    let top = metadata.children(Atom {
        kind: *b"file",
        data: 0,
        end,
    })?;
    let movie = metadata.children(required(&top, b"moov")?)?;
    for atom in movie.iter().filter(|atom| atom.kind == *b"trak") {
        let children = metadata.children(*atom)?;
        let header = required(&children, b"tkhd")?;
        let version = metadata.version(header)?;
        let id = u32::from_be_bytes(metadata.bytes(header, if version == 0 { 12 } else { 20 })?);
        if id != track {
            continue;
        }
        let Some(edits) = find(&children, b"edts") else {
            return Ok(None);
        };
        let edits = metadata.children(edits)?;
        let Some(edit) = find(&edits, b"elst") else {
            return Ok(None);
        };
        let version = metadata.version(edit)?;
        let count = u32::from_be_bytes(metadata.bytes(edit, 4)?);
        if count == 0 {
            return Ok(None);
        }
        if count != 1 {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "MP4 edit lists with multiple segments",
            ));
        }
        if find(&movie, b"mvex").is_some() {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "presentation edits in fragmented MP4 files",
            ));
        }
        let (duration, start, rate_offset) = if version == 0 {
            (
                u64::from(u32::from_be_bytes(metadata.bytes(edit, 8)?)),
                i64::from(i32::from_be_bytes(metadata.bytes(edit, 12)?)),
                16,
            )
        } else {
            (
                u64::from_be_bytes(metadata.bytes(edit, 8)?),
                i64::from_be_bytes(metadata.bytes(edit, 16)?),
                24,
            )
        };
        if start < 0 || metadata.bytes::<4>(edit, rate_offset)? != [0, 1, 0, 0] {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "empty, dwell or rate-changing MP4 edits",
            ));
        }
        let (movie_scale, _) = metadata.timing(required(&movie, b"mvhd")?)?;
        let media = metadata.children(required(&children, b"mdia")?)?;
        let (media_scale, media_duration) = metadata.timing(required(&media, b"mdhd")?)?;
        let delay =
            u64::try_from((start as u128 * u128::from(rate)).div_ceil(u128::from(media_scale)))
                .map_err(|_| invalid("MP4 encoder delay exceeds the supported range"))?;
        let media_frames = match media_frames {
            Some(frames) => frames,
            None => frames(media_duration, media_scale, rate)?,
        };
        let available = media_frames
            .checked_sub(delay)
            .ok_or_else(|| invalid("MP4 edit starts beyond the audio duration"))?;
        // Movie timing can be rounded to milliseconds; media timing retains sample precision.
        let frames = available.min(frames(duration, movie_scale, rate)?);
        return Ok(Some(Presentation {
            delay: u32::try_from(delay)
                .map_err(|_| invalid("MP4 encoder delay exceeds the supported range"))?,
            frames,
        }));
    }
    Err(invalid("MP4 audio track is missing"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn atom(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
        [(data.len() as u32 + 8).to_be_bytes().as_slice(), kind, data].concat()
    }

    fn timing(kind: &[u8; 4], version: u8, scale: u32, duration: u64) -> Vec<u8> {
        let mut data = vec![version, 0, 0, 0];
        data.extend(vec![0; if version == 0 { 8 } else { 16 }]);
        data.extend(scale.to_be_bytes());
        if version == 0 {
            data.extend((duration as u32).to_be_bytes());
        } else {
            data.extend(duration.to_be_bytes());
        }
        atom(kind, &data)
    }

    fn track(version: u8, id: u32, start: i64, duration: u64, scale: u32) -> Vec<u8> {
        let mut header = vec![version, 0, 0, 0];
        header.extend(vec![0; if version == 0 { 8 } else { 16 }]);
        header.extend(id.to_be_bytes());
        let mut edit = vec![version, 0, 0, 0];
        edit.extend(1u32.to_be_bytes());
        if version == 0 {
            edit.extend((duration as u32).to_be_bytes());
            edit.extend((start as i32).to_be_bytes());
        } else {
            edit.extend(duration.to_be_bytes());
            edit.extend(start.to_be_bytes());
        }
        edit.extend([0, 1, 0, 0]);
        atom(
            b"trak",
            &[
                atom(b"tkhd", &header),
                atom(b"edts", &atom(b"elst", &edit)),
                atom(b"mdia", &timing(b"mdhd", version, scale, 45_141)),
            ]
            .concat(),
        )
    }

    fn read(bytes: &[u8], media_frames: Option<u64>) -> io::Result<Option<Presentation>> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("metadata.m4a");
        std::fs::write(&path, bytes)?;
        presentation(&path, 7, 44_100, media_frames)
    }

    #[test]
    fn edit_versions_select_the_audio_track_and_keep_sample_precision() {
        for version in [0, 1] {
            let bytes = atom(
                b"moov",
                &[
                    timing(b"mvhd", version, 1000, 1001),
                    track(version, 99, -1, 1001, 44_100),
                    track(version, 7, 1024, 1001, 44_100),
                ]
                .concat(),
            );
            for media_frames in [None, Some(45_141)] {
                let result = read(&bytes, media_frames).unwrap().unwrap();
                assert_eq!(result.delay, 1024);
                assert_eq!(result.frames, 44_117);
            }
        }
    }

    #[test]
    fn non_sample_timescales_round_start_up_and_bound_the_edited_end() {
        let bytes = atom(
            b"moov",
            &[timing(b"mvhd", 0, 1000, 500), track(0, 7, 23, 500, 1000)].concat(),
        );
        let result = read(&bytes, None).unwrap().unwrap();
        assert_eq!(result.delay, 1015);
        assert_eq!(result.frames, 22_050);
    }

    #[test]
    fn malformed_metadata_is_bounded_and_unsupported_edits_fail_explicitly() {
        for start in [-1, 100_000] {
            let bytes = atom(
                b"moov",
                &[
                    timing(b"mvhd", 0, 1000, 1000),
                    track(0, 7, start, 1000, 44_100),
                ]
                .concat(),
            );
            let error = read(&bytes, Some(45_124)).err().unwrap();
            assert_eq!(
                error.kind(),
                if start < 0 {
                    io::ErrorKind::Unsupported
                } else {
                    io::ErrorKind::InvalidData
                }
            );
        }
        let many = atom(b"free", &[]).repeat(4097);
        assert_eq!(
            read(&many, None).err().unwrap().kind(),
            io::ErrorKind::InvalidData
        );
        for bytes in [
            vec![0; 7],
            [u32::MAX.to_be_bytes().as_slice(), b"moov"].concat(),
        ] {
            assert_eq!(
                read(&bytes, None).err().unwrap().kind(),
                io::ErrorKind::InvalidData
            );
        }
    }
}
