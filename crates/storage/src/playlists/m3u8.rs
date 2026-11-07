use std::path::{Component, Path, PathBuf};

use parabolica_domain::ids::{PlaylistEntryId, PlaylistId};
use parabolica_domain::source::PlayableSource;

use super::files::{absolute_path, register_file_in, source_details_in};
use super::{
    PLAYLIST_ENTRY_LIMIT, append_in, create_in, entries_in, playlist_name, require_playlist,
};
use crate::{Storage, StorageError};

pub const IMPORT_BYTE_LIMIT: usize = 8 * 1024 * 1024;
pub const IMPORT_LINE_LIMIT: usize = 32_768;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportProblem {
    pub entry: PlaylistEntryId,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct M3u8Export {
    pub text: String,
    /// An exporter must show these entries before writing a partial export.
    pub non_exportable: Vec<ExportProblem>,
}

impl Storage {
    /// UTF-8 only, optional BOM and CRLF accepted. Paths resolve relative to the playlist file.
    /// Up to 8 MiB, 10000 entries and 32768 bytes per line. URLs and NUL are rejected.
    /// Parsing, path identities and playlist insertion succeed together or leave no writes.
    pub fn import_m3u8(
        &self,
        name: &str,
        playlist_file: &Path,
        bytes: &[u8],
    ) -> Result<PlaylistId, StorageError> {
        let name = playlist_name(name)?;
        let playlist_file = absolute_path(playlist_file)?;
        let directory = playlist_file
            .parent()
            .ok_or(StorageError::invalid("playlist path", "has no parent"))?;
        let paths = parse(bytes, directory)?;
        self.write(move |tx| {
            let id = create_in(tx, &name)?;
            let sources = paths
                .iter()
                .map(|path| register_file_in(tx, path).map(PlayableSource::LocalFile))
                .collect::<Result<Vec<_>, _>>()?;
            append_in(tx, id, &sources)?;
            Ok(id)
        })
    }

    /// Local whole-file sources export as relative Unicode paths when possible, including missing
    /// paths. CUE ranges and provider identities cannot be represented by M3U8 and are reported.
    pub fn export_m3u8(
        &self,
        playlist: PlaylistId,
        destination: &Path,
    ) -> Result<M3u8Export, StorageError> {
        let destination = absolute_path(destination)?;
        self.read(move |tx| {
            require_playlist(tx, playlist)?;
            let directory = destination
                .parent()
                .ok_or(StorageError::invalid("playlist path", "has no parent"))?;
            let mut export = M3u8Export {
                text: "#EXTM3U\n".into(),
                non_exportable: Vec::new(),
            };
            for entry in entries_in(tx, playlist)? {
                let path = match &entry.source {
                    PlayableSource::LocalFile(_) => source_details_in(tx, &entry.source)?.path,
                    PlayableSource::CueTrack(_) => {
                        export.non_exportable.push(ExportProblem {
                            entry: entry.id,
                            reason: "M3U8 cannot preserve a CUE track range".into(),
                        });
                        continue;
                    }
                    PlayableSource::Provider(_) => {
                        export.non_exportable.push(ExportProblem {
                            entry: entry.id,
                            reason: "M3U8 cannot preserve a provider identity".into(),
                        });
                        continue;
                    }
                };
                let Some(path) = path else {
                    export.non_exportable.push(ExportProblem {
                        entry: entry.id,
                        reason: "File path is no longer stored".into(),
                    });
                    continue;
                };
                let path = match absolute_path(&path) {
                    Ok(path) => path,
                    Err(_) => {
                        export.non_exportable.push(ExportProblem {
                            entry: entry.id,
                            reason: "File path is not safe for M3U8".into(),
                        });
                        continue;
                    }
                };
                let relative = relative_path(&path, directory);
                let text = relative
                    .to_str()
                    .ok_or(StorageError::invalid("file path", "is not Unicode"))?;
                #[cfg(windows)]
                let text = &text.replace('\\', "/");
                if text.starts_with('#') {
                    export.text.push_str("./");
                }
                export.text.push_str(text);
                export.text.push('\n');
            }
            Ok(export)
        })
    }
}

fn parse(bytes: &[u8], directory: &Path) -> Result<Vec<PathBuf>, StorageError> {
    if bytes.len() > IMPORT_BYTE_LIMIT {
        return Err(StorageError::invalid("M3U8", "exceeds 8 MiB"));
    }
    let text =
        std::str::from_utf8(bytes).map_err(|_| StorageError::invalid("M3U8", "is not UTF-8"))?;
    let mut paths = Vec::new();
    for line in text.strip_prefix('\u{feff}').unwrap_or(text).split('\n') {
        if line.len() > IMPORT_LINE_LIMIT {
            return Err(StorageError::invalid("M3U8 line", "exceeds 32768 bytes"));
        }
        let line = line.strip_suffix('\r').unwrap_or(line);
        if line.contains(['\r', '\0']) {
            return Err(StorageError::invalid(
                "M3U8 line",
                "contains a line break or NUL",
            ));
        }
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.contains("://") || line.starts_with("file:") {
            return Err(StorageError::invalid(
                "M3U8",
                "URLs are not supported; use file paths",
            ));
        }
        paths.push(absolute_path(&directory.join(line))?);
        if paths.len() > PLAYLIST_ENTRY_LIMIT {
            return Err(StorageError::invalid("M3U8", "exceeds 10000 entries"));
        }
    }
    Ok(paths)
}

fn relative_path(path: &Path, base: &Path) -> PathBuf {
    let path_parts: Vec<_> = path.components().collect();
    let base_parts: Vec<_> = base.components().collect();
    let shared = path_parts
        .iter()
        .zip(&base_parts)
        .take_while(|(left, right)| left == right)
        .count();
    if shared == 0
        || base_parts[shared..]
            .iter()
            .any(|part| !matches!(part, Component::Normal(_)))
    {
        return path.to_owned();
    }
    let mut relative = PathBuf::new();
    for _ in &base_parts[shared..] {
        relative.push("..");
    }
    for part in &path_parts[shared..] {
        relative.push(part.as_os_str());
    }
    relative
}
