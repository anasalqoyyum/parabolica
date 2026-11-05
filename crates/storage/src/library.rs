//! Library roots, physical file identity, display tags, CUE ranges and library search.
//!
//! File identity (location, OS file ID, signature) and the display tags read from the file are
//! stored separately: rewriting tags never changes a file's identity, and identity survives a move.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use parabolica_domain::cue::VirtualTrackRange;
use parabolica_domain::files::{FileLocation, FileSignature, OsFileId, RootAvailability, VolumeId};
use parabolica_domain::ids::{LibraryRootId, MediaFileId, VirtualTrackId};
use rusqlite::types::Value;
use rusqlite::{OptionalExtension, Transaction, params, params_from_iter};
use unicode_normalization::UnicodeNormalization;

use crate::codec::{
    blob16, now_ms, optional_blob16, path_to_sql, relative_path_from_sql, relative_path_to_sql,
    signature_from_sql, signature_to_sql, sql_to_u64, u64_to_sql,
};
use crate::{Storage, StorageError};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibraryRoot {
    pub id: LibraryRootId,
    pub path: PathBuf,
    pub volume: Option<VolumeId>,
    pub availability: RootAvailability,
}

/// One tag value as read from a file. Multi-value fields repeat the field in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DisplayTag {
    /// Vorbis-comment style field name, stored upper-case (`TITLE`, `ALBUMARTIST`).
    pub field: String,
    pub value: String,
}

impl DisplayTag {
    pub fn new(field: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            field: field.into(),
            value: value.into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Presence {
    Present,
    /// Not found at its location. The record, its links and analysis stay until removed.
    Missing,
}

/// What an index pass observed for one file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaFileRecord {
    pub location: FileLocation,
    pub os_file_id: Option<OsFileId>,
    pub signature: FileSignature,
    pub tags: Vec<DisplayTag>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaFile {
    pub id: MediaFileId,
    pub location: FileLocation,
    pub os_file_id: Option<OsFileId>,
    pub signature: FileSignature,
    pub presence: Presence,
}

const SEARCH_TITLE: &str = "TITLE";
const SEARCH_ARTIST: &str = "ARTIST";
const SEARCH_ALBUM: &str = "ALBUM";
const SEARCH_ALBUM_ARTIST: &str = "ALBUMARTIST";
const SEARCH_OTHER: [&str; 6] = [
    "GENRE",
    "STYLE",
    "COMPOSER",
    "PERFORMER",
    "LABEL",
    "CATALOGNUMBER",
];

impl Storage {
    /// Add a root, or return the existing root at the same path. The caller canonicalizes `path`.
    pub fn add_library_root(
        &self,
        path: &Path,
        volume: Option<VolumeId>,
    ) -> Result<LibraryRootId, StorageError> {
        let path = path_to_sql(path)?.to_owned();
        if path.is_empty() {
            return Err(StorageError::invalid("library root", "has an empty path"));
        }
        self.write(move |tx| {
            let existing = tx
                .query_row(
                    "SELECT id FROM library_roots WHERE path = ?1",
                    [&path],
                    |row| row.get::<_, Vec<u8>>(0),
                )
                .optional()?;
            if let Some(id) = existing {
                let id: [u8; 16] = id
                    .try_into()
                    .map_err(|_| StorageError::InvalidStoredValue {
                        column: "library_roots.id",
                    })?;
                return Ok(LibraryRootId::from_bytes(id));
            }
            let id = LibraryRootId::generate();
            tx.execute(
                "INSERT INTO library_roots (id, path, volume_id, availability, added_at_ms)
                 VALUES (?1, ?2, ?3, 'online', ?4)",
                params![id.to_bytes(), path, volume.map(|volume| volume.0), now_ms()],
            )?;
            Ok(id)
        })
    }

    pub fn library_roots(&self) -> Result<Vec<LibraryRoot>, StorageError> {
        self.read(|tx| {
            let mut statement = tx.prepare(
                "SELECT id, path, volume_id, availability FROM library_roots ORDER BY added_at_ms, path",
            )?;
            let mut rows = statement.query([])?;
            let mut roots = Vec::new();
            while let Some(row) = rows.next()? {
                roots.push(LibraryRoot {
                    id: LibraryRootId::from_bytes(blob16(row, 0, "library_roots.id")?),
                    path: PathBuf::from(row.get::<_, String>(1)?),
                    volume: row.get::<_, Option<String>>(2)?.map(VolumeId),
                    availability: availability_from_sql(&row.get::<_, String>(3)?)?,
                });
            }
            Ok(roots)
        })
    }

    /// Record a root going offline or losing permission. Its files keep their records.
    pub fn set_root_availability(
        &self,
        root: LibraryRootId,
        availability: RootAvailability,
    ) -> Result<(), StorageError> {
        self.write(move |tx| {
            let changed = tx.execute(
                "UPDATE library_roots SET availability = ?2 WHERE id = ?1",
                params![root.to_bytes(), availability_to_sql(availability)],
            )?;
            expect_one(changed, root)
        })
    }

    /// Remove a root and every file record under it. Queue entries keep their sources and show
    /// them as unavailable.
    pub fn remove_library_root(&self, root: LibraryRootId) -> Result<(), StorageError> {
        self.write(move |tx| {
            let changed =
                tx.execute("DELETE FROM library_roots WHERE id = ?1", [root.to_bytes()])?;
            expect_one(changed, root)
        })
    }

    /// Insert or update a file and replace its display tags in one transaction.
    ///
    /// Identity is kept for a file already recorded at the same location. Otherwise a missing
    /// file with the same OS file ID is treated as moved and keeps its identity. Anything else is a
    /// new file. Ambiguous moves without an OS file ID go through [`Storage::move_media_file`].
    pub fn record_media_file(&self, record: MediaFileRecord) -> Result<MediaFileId, StorageError> {
        let relative_path = relative_path_to_sql(&record.location.relative_path)?;
        let signature = signature_to_sql(&record.signature)?;
        let tags = normalized_tags(record.tags)?;
        let root = record.location.root;
        let os_identity = record
            .os_file_id
            .map(|os| (os.volume.0, os.file_id.to_be_bytes()));
        self.write(move |tx| {
            let at_location = tx
                .query_row(
                    "SELECT id FROM media_files WHERE root_id = ?1 AND relative_path = ?2",
                    params![root.to_bytes(), relative_path],
                    |row| row.get::<_, Vec<u8>>(0),
                )
                .optional()?;
            let moved = match (&at_location, &os_identity) {
                (None, Some((volume, file_id))) => {
                    let mut statement = tx.prepare(
                        "SELECT id FROM media_files
                         WHERE os_volume_id = ?1 AND os_file_id = ?2 AND presence = 'missing'",
                    )?;
                    let candidates: Vec<Vec<u8>> = statement
                        .query_map(params![volume, file_id], |row| row.get(0))?
                        .collect::<Result<_, _>>()?;
                    match <[_; 1]>::try_from(candidates) {
                        Ok([id]) => Some(id),
                        Err(_) => None,
                    }
                }
                _ => None,
            };
            let id = match at_location.or(moved) {
                Some(bytes) => MediaFileId::from_bytes(bytes.try_into().map_err(|_| {
                    StorageError::InvalidStoredValue {
                        column: "media_files.id",
                    }
                })?),
                None => MediaFileId::generate(),
            };
            let (os_volume, os_file_id) = os_identity.unzip();
            tx.execute(
                "INSERT INTO media_files (
                     id, root_id, relative_path, os_volume_id, os_file_id, size_bytes,
                     modified_unix_nanos, content_digest, presence, indexed_at_ms)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'present', ?9)
                 ON CONFLICT (id) DO UPDATE SET
                     root_id = excluded.root_id,
                     relative_path = excluded.relative_path,
                     os_volume_id = excluded.os_volume_id,
                     os_file_id = excluded.os_file_id,
                     size_bytes = excluded.size_bytes,
                     modified_unix_nanos = excluded.modified_unix_nanos,
                     content_digest = excluded.content_digest,
                     presence = 'present',
                     indexed_at_ms = excluded.indexed_at_ms",
                params![
                    id.to_bytes(),
                    root.to_bytes(),
                    relative_path,
                    os_volume,
                    os_file_id,
                    signature.size_bytes,
                    signature.modified_unix_nanos,
                    signature.digest,
                    now_ms(),
                ],
            )?;
            write_tags(tx, id, &tags)?;
            index_for_search(tx, id)?;
            Ok(id)
        })
    }

    /// Give a file a new location, keeping its identity, tags, links and job history.
    pub fn move_media_file(
        &self,
        file: MediaFileId,
        location: FileLocation,
    ) -> Result<(), StorageError> {
        let relative_path = relative_path_to_sql(&location.relative_path)?;
        let root = location.root;
        self.write(move |tx| {
            let changed = tx.execute(
                "UPDATE media_files SET root_id = ?2, relative_path = ?3 WHERE id = ?1",
                params![file.to_bytes(), root.to_bytes(), relative_path],
            )?;
            expect_one(changed, file)?;
            index_for_search(tx, file)
        })
    }

    pub fn set_media_file_presence(
        &self,
        file: MediaFileId,
        presence: Presence,
    ) -> Result<(), StorageError> {
        self.write(move |tx| {
            let changed = tx.execute(
                "UPDATE media_files SET presence = ?2 WHERE id = ?1",
                params![file.to_bytes(), presence_to_sql(presence)],
            )?;
            expect_one(changed, file)
        })
    }

    pub fn media_file(&self, file: MediaFileId) -> Result<Option<MediaFile>, StorageError> {
        self.read(move |tx| {
            let mut statement = tx.prepare(
                "SELECT root_id, relative_path, os_volume_id, os_file_id, size_bytes,
                        modified_unix_nanos, content_digest, presence
                 FROM media_files WHERE id = ?1",
            )?;
            let mut rows = statement.query([file.to_bytes()])?;
            let Some(row) = rows.next()? else {
                return Ok(None);
            };
            let os_file_id = match (
                row.get::<_, Option<String>>(2)?,
                optional_blob16(row, 3, "media_files.os_file_id")?,
            ) {
                (Some(volume), Some(file_id)) => Some(OsFileId {
                    volume: VolumeId(volume),
                    file_id: u128::from_be_bytes(file_id),
                }),
                _ => None,
            };
            Ok(Some(MediaFile {
                id: file,
                location: FileLocation {
                    root: LibraryRootId::from_bytes(blob16(row, 0, "media_files.root_id")?),
                    relative_path: relative_path_from_sql(&row.get::<_, String>(1)?),
                },
                os_file_id,
                signature: signature_from_sql(row.get(4)?, row.get(5)?, row.get(6)?)?,
                presence: presence_from_sql(&row.get::<_, String>(7)?)?,
            }))
        })
    }

    pub fn media_file_tags(&self, file: MediaFileId) -> Result<Vec<DisplayTag>, StorageError> {
        self.read(move |tx| {
            let mut statement = tx.prepare(
                "SELECT field, value FROM media_file_tags WHERE file_id = ?1 ORDER BY field, ordinal",
            )?;
            let tags = statement
                .query_map([file.to_bytes()], |row| {
                    Ok(DisplayTag {
                        field: row.get(0)?,
                        value: row.get(1)?,
                    })
                })?
                .collect::<Result<_, _>>()?;
            Ok(tags)
        })
    }

    /// Set the CUE tracks of one image. Tracks whose IDs are absent from `ranges` are removed with
    /// their source links; kept IDs keep their links.
    pub fn replace_virtual_tracks(
        &self,
        image: MediaFileId,
        ranges: Vec<VirtualTrackRange>,
    ) -> Result<(), StorageError> {
        if ranges.iter().any(|range| range.image != image) {
            return Err(StorageError::invalid(
                "virtual tracks",
                "belong to a different image",
            ));
        }
        let mut numbers = HashSet::new();
        let mut ids = HashSet::new();
        if !ranges
            .iter()
            .all(|range| numbers.insert(range.number) && ids.insert(range.id))
        {
            return Err(StorageError::invalid(
                "virtual tracks",
                "repeat a track number or ID",
            ));
        }
        self.write(move |tx| {
            let kept: Vec<[u8; 16]> = ranges.iter().map(|range| range.id.to_bytes()).collect();
            let mut existing = tx.prepare("SELECT id FROM virtual_tracks WHERE image_file_id = ?1")?;
            let existing: Vec<Vec<u8>> = existing
                .query_map([image.to_bytes()], |row| row.get(0))?
                .collect::<Result<_, _>>()?;
            for id in existing {
                if !kept.iter().any(|kept| kept.as_slice() == id.as_slice()) {
                    tx.execute("DELETE FROM virtual_tracks WHERE id = ?1", [id])?;
                }
            }
            for range in &ranges {
                tx.execute(
                    "INSERT INTO virtual_tracks (id, image_file_id, number, start_frame, end_frame, index_01_frame)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                     ON CONFLICT (id) DO UPDATE SET
                         number = excluded.number,
                         start_frame = excluded.start_frame,
                         end_frame = excluded.end_frame,
                         index_01_frame = excluded.index_01_frame
                     WHERE image_file_id = excluded.image_file_id",
                    params![
                        range.id.to_bytes(),
                        image.to_bytes(),
                        range.number,
                        u64_to_sql(range.frames.start, "virtual track start")?,
                        u64_to_sql(range.frames.end, "virtual track end")?,
                        u64_to_sql(range.index_01, "virtual track INDEX 01")?,
                    ],
                )?;
            }
            Ok(())
        })
    }

    pub fn virtual_tracks(
        &self,
        image: MediaFileId,
    ) -> Result<Vec<VirtualTrackRange>, StorageError> {
        self.read(move |tx| {
            let mut statement = tx.prepare(
                "SELECT id, number, start_frame, end_frame, index_01_frame
                 FROM virtual_tracks WHERE image_file_id = ?1 ORDER BY number",
            )?;
            let mut rows = statement.query([image.to_bytes()])?;
            let mut ranges = Vec::new();
            while let Some(row) = rows.next()? {
                ranges.push(VirtualTrackRange {
                    id: VirtualTrackId::from_bytes(blob16(row, 0, "virtual_tracks.id")?),
                    image,
                    number: row.get(1)?,
                    frames: sql_to_u64(row.get(2)?, "virtual_tracks.start_frame")?
                        ..sql_to_u64(row.get(3)?, "virtual_tracks.end_frame")?,
                    index_01: sql_to_u64(row.get(4)?, "virtual_tracks.index_01_frame")?,
                });
            }
            Ok(ranges)
        })
    }

    /// Files whose title, artists, album, genre, label or file name contain every word of
    /// `query`, ignoring case and diacritics. Words of three or more characters use the trigram
    /// index; shorter words are matched against the folded text of the indexed matches (or of every
    /// file when the query has no longer word).
    pub fn search_media_files(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<MediaFileId>, StorageError> {
        let mut phrases = Vec::new();
        let mut short_words = Vec::new();
        for word in query.split_whitespace().map(fold_for_search) {
            // Counted after folding: a decomposed accent is not a character the index sees.
            if word.chars().count() >= 3 {
                phrases.push(format!("\"{}\"", word.replace('"', "\"\"")));
            } else {
                short_words.push(word);
            }
        }
        if phrases.is_empty() && short_words.is_empty() {
            return Ok(Vec::new());
        }
        let limit = i64::try_from(limit).unwrap_or(i64::MAX);

        let ranked = !phrases.is_empty();
        let mut conditions = Vec::new();
        let mut values: Vec<Value> = Vec::new();
        if ranked {
            conditions.push("s.media_file_search MATCH ?".to_owned());
            values.push(Value::Text(phrases.join(" AND ")));
        }
        for word in short_words {
            conditions.push("instr(s.folded, ?) > 0".to_owned());
            values.push(Value::Text(word));
        }
        values.push(Value::Integer(limit));
        let order = if ranked { "s.rank, f.seq" } else { "f.seq" };
        let sql = format!(
            "SELECT f.id FROM media_file_search s JOIN media_files f ON f.seq = s.rowid
             WHERE {} ORDER BY {order} LIMIT ?",
            conditions.join(" AND ")
        );
        self.read(move |tx| {
            let mut statement = tx.prepare(&sql)?;
            let mut rows = statement.query(params_from_iter(values))?;
            let mut ids = Vec::new();
            while let Some(row) = rows.next()? {
                ids.push(MediaFileId::from_bytes(blob16(row, 0, "media_files.id")?));
            }
            Ok(ids)
        })
    }
}

fn normalized_tags(tags: Vec<DisplayTag>) -> Result<Vec<DisplayTag>, StorageError> {
    tags.into_iter()
        .map(|tag| {
            let field = tag.field.trim().to_ascii_uppercase();
            if field.is_empty() {
                return Err(StorageError::invalid("tag field", "is empty"));
            }
            Ok(DisplayTag {
                field,
                value: tag.value,
            })
        })
        .collect()
}

fn write_tags(
    tx: &Transaction<'_>,
    file: MediaFileId,
    tags: &[DisplayTag],
) -> Result<(), StorageError> {
    tx.execute(
        "DELETE FROM media_file_tags WHERE file_id = ?1",
        [file.to_bytes()],
    )?;
    let mut insert = tx.prepare(
        "INSERT INTO media_file_tags (file_id, field, ordinal, value) VALUES (?1, ?2, ?3, ?4)",
    )?;
    let mut ordinals: Vec<(&str, i64)> = Vec::new();
    for tag in tags {
        let ordinal = match ordinals.iter_mut().find(|(field, _)| *field == tag.field) {
            Some((_, next)) => {
                *next += 1;
                *next
            }
            None => {
                ordinals.push((&tag.field, 0));
                0
            }
        };
        insert.execute(params![file.to_bytes(), tag.field, ordinal, tag.value])?;
    }

    Ok(())
}

/// Rewrite the search row of `file` from its stored tags and file name.
fn index_for_search(tx: &Transaction<'_>, file: MediaFileId) -> Result<(), StorageError> {
    let (seq, relative_path): (i64, String) = tx.query_row(
        "SELECT seq, relative_path FROM media_files WHERE id = ?1",
        [file.to_bytes()],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    let mut statement = tx.prepare(
        "SELECT field, value FROM media_file_tags WHERE file_id = ?1 ORDER BY field, ordinal",
    )?;
    let tags: Vec<(String, String)> = statement
        .query_map([file.to_bytes()], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect::<Result<_, _>>()?;
    let joined = |fields: &[&str]| {
        tags.iter()
            .filter(|(field, _)| fields.contains(&field.as_str()))
            .map(|(_, value)| value.as_str())
            .collect::<Vec<_>>()
            .join(" ")
    };
    let file_name = relative_path_from_sql(&relative_path)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or_default()
        .to_owned();
    let columns = [
        joined(&[SEARCH_TITLE]),
        joined(&[SEARCH_ARTIST]),
        joined(&[SEARCH_ALBUM]),
        joined(&[SEARCH_ALBUM_ARTIST]),
        format!("{} {file_name}", joined(&SEARCH_OTHER))
            .trim()
            .to_owned(),
    ];
    let folded = fold_for_search(&columns.join(" "));
    tx.execute("DELETE FROM media_file_search WHERE rowid = ?1", [seq])?;
    tx.execute(
        "INSERT INTO media_file_search (rowid, title, artist, album, album_artist, other, folded)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            seq, columns[0], columns[1], columns[2], columns[3], columns[4], folded
        ],
    )?;
    Ok(())
}

/// Lower-case `text` and drop Latin-script accents (combining diacritical marks after canonical
/// decomposition), matching what the trigram tokenizer ignores. Other marks, such as kana voicing,
/// are kept.
fn fold_for_search(text: &str) -> String {
    text.nfd()
        .filter(|c| !('\u{0300}'..='\u{036f}').contains(c))
        .flat_map(char::to_lowercase)
        .collect()
}

pub(crate) fn expect_one(changed: usize, id: impl std::fmt::Display) -> Result<(), StorageError> {
    if changed == 0 {
        Err(StorageError::NotFound(id.to_string()))
    } else {
        Ok(())
    }
}

fn availability_to_sql(availability: RootAvailability) -> &'static str {
    match availability {
        RootAvailability::Online => "online",
        RootAvailability::Offline => "offline",
        RootAvailability::PermissionDenied => "permission-denied",
    }
}

fn availability_from_sql(text: &str) -> Result<RootAvailability, StorageError> {
    match text {
        "online" => Ok(RootAvailability::Online),
        "offline" => Ok(RootAvailability::Offline),
        "permission-denied" => Ok(RootAvailability::PermissionDenied),
        _ => Err(StorageError::InvalidStoredValue {
            column: "library_roots.availability",
        }),
    }
}

fn presence_to_sql(presence: Presence) -> &'static str {
    match presence {
        Presence::Present => "present",
        Presence::Missing => "missing",
    }
}

fn presence_from_sql(text: &str) -> Result<Presence, StorageError> {
    match text {
        "present" => Ok(Presence::Present),
        "missing" => Ok(Presence::Missing),
        _ => Err(StorageError::InvalidStoredValue {
            column: "media_files.presence",
        }),
    }
}
