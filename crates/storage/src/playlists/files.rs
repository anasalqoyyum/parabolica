use std::path::{Component, Path, PathBuf};

use parabolica_domain::ids::{MediaFileId, VirtualTrackId};
use parabolica_domain::source::{FrameSpan, PlayableSource, ResolvedSource};
use rusqlite::{OptionalExtension, Transaction, params};

use crate::codec::{blob16, now_ms, sql_to_u64};
use crate::{Storage, StorageError};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceDetails {
    pub source: PlayableSource,
    pub title: String,
    pub source_label: String,
    pub path: Option<PathBuf>,
    pub unavailable_reason: Option<String>,
    pub resolved: Option<ResolvedSource>,
}

impl Storage {
    /// Reuse indexed identity at this exact path, otherwise keep an app-owned external-file ID.
    /// Missing files are recorded too. This never creates library roots or changes indexed files.
    pub fn register_file_path(&self, path: &Path) -> Result<MediaFileId, StorageError> {
        let path = absolute_path(path)?;
        self.write(move |tx| register_file_in(tx, &path))
    }

    pub fn source_details(&self, source: &PlayableSource) -> Result<SourceDetails, StorageError> {
        let source = source.clone();
        self.read(move |tx| source_details_in(tx, &source))
    }
}

pub(super) fn absolute_path(path: &Path) -> Result<PathBuf, StorageError> {
    let text = path
        .to_str()
        .ok_or(StorageError::invalid("file path", "is not Unicode"))?;
    if text.is_empty() || text.len() > 32_768 || text.contains(['\n', '\r', '\0']) {
        return Err(StorageError::invalid(
            "file path",
            "is empty, too long or contains a line break or NUL",
        ));
    }
    let absolute = if path.is_absolute() {
        path.to_owned()
    } else {
        std::env::current_dir()?.join(path)
    };
    let mut normalized = PathBuf::new();
    for component in absolute.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            other => normalized.push(other.as_os_str()),
        }
    }
    Ok(normalized)
}

pub(super) fn register_file_in(
    tx: &Transaction<'_>,
    path: &Path,
) -> Result<MediaFileId, StorageError> {
    let mut statement = tx.prepare("SELECT id, path FROM library_roots")?;
    let mut rows = statement.query([])?;
    while let Some(row) = rows.next()? {
        let root_id = blob16(row, 0, "library_roots.id")?;
        let root: String = row.get(1)?;
        let root = absolute_path(Path::new(&root))?;
        let Ok(relative) = path.strip_prefix(root) else {
            continue;
        };
        let relative = crate::codec::relative_path_to_sql(relative)?;
        let mut indexed =
            tx.prepare("SELECT id FROM media_files WHERE root_id = ?1 AND relative_path = ?2")?;
        let mut indexed_rows = indexed.query(params![root_id, relative])?;
        if let Some(row) = indexed_rows.next()? {
            return Ok(MediaFileId::from_bytes(blob16(row, 0, "media_files.id")?));
        }
    }
    let text = path
        .to_str()
        .ok_or(StorageError::invalid("file path", "is not Unicode"))?;
    let existing = tx
        .query_row(
            "SELECT id FROM external_files WHERE path = ?1",
            [text],
            |row| row.get::<_, Vec<u8>>(0),
        )
        .optional()?;
    if let Some(id) = existing {
        return Ok(MediaFileId::from_bytes(id.try_into().map_err(|_| {
            StorageError::InvalidStoredValue {
                column: "external_files.id",
            }
        })?));
    }
    let id = MediaFileId::generate();
    tx.execute(
        "INSERT INTO external_files (id, path, added_at_ms) VALUES (?1, ?2, ?3)",
        params![id.to_bytes(), text, now_ms()],
    )?;
    Ok(id)
}

pub(super) fn source_details_in(
    tx: &Transaction<'_>,
    source: &PlayableSource,
) -> Result<SourceDetails, StorageError> {
    match source {
        PlayableSource::Provider(item) => Ok(SourceDetails {
            source: source.clone(),
            title: item.item.0.clone(),
            source_label: item.provider.key().to_owned(),
            path: None,
            resolved: None,
            unavailable_reason: Some("Provider playback is not connected".into()),
        }),
        PlayableSource::LocalFile(file) => file_details(tx, *file, source.clone()),
        PlayableSource::CueTrack(track) => cue_details(tx, *track),
    }
}

fn file_details(
    tx: &Transaction<'_>,
    file: MediaFileId,
    source: PlayableSource,
) -> Result<SourceDetails, StorageError> {
    let indexed: Option<(String, String, String, String, Option<String>)> = tx.query_row(
        "SELECT r.path, f.relative_path, r.availability, f.presence,
            (SELECT value FROM media_file_tags WHERE file_id = f.id AND field = 'TITLE' ORDER BY ordinal LIMIT 1)
         FROM media_files f JOIN library_roots r ON r.id = f.root_id WHERE f.id = ?1", [file.to_bytes()],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?))).optional()?;
    let (path, title, unavailable_reason) =
        if let Some((root, relative, availability, presence, title)) = indexed {
            let reason = match (availability.as_str(), presence.as_str()) {
                ("offline", _) => Some("Library folder is offline".into()),
                ("permission-denied", _) => Some("Library folder access was denied".into()),
                (_, "missing") => Some("File is missing from the library folder".into()),
                _ => None,
            };
            (Some(PathBuf::from(root).join(relative)), title, reason)
        } else {
            let path: Option<String> = tx
                .query_row(
                    "SELECT path FROM external_files WHERE id = ?1",
                    [file.to_bytes()],
                    |row| row.get(0),
                )
                .optional()?;
            let reason = path
                .is_none()
                .then(|| "File identity is no longer stored".to_owned());
            (path.map(PathBuf::from), None, reason)
        };
    let title = title
        .or_else(|| {
            path.as_ref()
                .and_then(|path| path.file_stem()?.to_str().map(str::to_owned))
        })
        .unwrap_or_else(|| file.to_string());
    let resolved = path
        .clone()
        .filter(|_| unavailable_reason.is_none())
        .map(|path| ResolvedSource::local_file(file, path));
    Ok(SourceDetails {
        source,
        title,
        source_label: "Local file".into(),
        path,
        unavailable_reason,
        resolved,
    })
}

fn cue_details(tx: &Transaction<'_>, track: VirtualTrackId) -> Result<SourceDetails, StorageError> {
    let mut statement = tx.prepare(
        "SELECT image_file_id, number, start_frame, end_frame FROM virtual_tracks WHERE id = ?1",
    )?;
    let mut rows = statement.query([track.to_bytes()])?;
    let source = PlayableSource::CueTrack(track);
    let Some(row) = rows.next()? else {
        return Ok(SourceDetails {
            source,
            title: track.to_string(),
            source_label: "CUE track".into(),
            path: None,
            resolved: None,
            unavailable_reason: Some("CUE track identity is no longer stored".into()),
        });
    };
    let file = MediaFileId::from_bytes(blob16(row, 0, "virtual_tracks.image_file_id")?);
    let number: u8 = row.get(1)?;
    let start = sql_to_u64(row.get(2)?, "virtual_tracks.start_frame")?;
    let end = sql_to_u64(row.get(3)?, "virtual_tracks.end_frame")?;
    let mut details = file_details(tx, file, source)?;
    details.title = format!("{} · Track {number}", details.title);
    details.source_label = "CUE track".into();
    if let Some(ResolvedSource::Local { span, .. }) = &mut details.resolved {
        *span = FrameSpan::Range(start..end);
    }
    Ok(details)
}
