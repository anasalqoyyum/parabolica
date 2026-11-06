//! Stored collection projections and paged browsing. All SQL stays with the storage owner.

use std::path::{Path, PathBuf};

use parabolica_domain::files::{FileLocation, OsFileId, VolumeId};
use parabolica_domain::ids::{ArtistId, LibraryRootId, MediaFileId, ReleaseId};
use rusqlite::{OptionalExtension, params, params_from_iter, types::Value};

use crate::codec::{
    blob16, path_to_sql, relative_path_from_sql, relative_path_to_sql, signature_from_sql,
};
use crate::library::{MediaFile, Presence, expect_one};
use crate::{Storage, StorageError};

#[derive(Debug, Clone)]
pub struct SongMetadata {
    pub group_key: String,
    pub album: String,
    pub album_artist: String,
    pub edition: String,
    pub title: String,
    pub artists: Vec<String>,
    pub folder: PathBuf,
    pub disc: u32,
    pub track: u32,
    pub duration_ms: u64,
    pub format: String,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrowseKind {
    Albums,
    Artists,
    Songs,
    Folders,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Sort {
    #[default]
    Title,
    Artist,
    RecentlyAdded,
    TrackOrder,
}
#[derive(Debug, Clone, Default)]
pub struct BrowseQuery {
    pub text: String,
    pub root: Option<LibraryRootId>,
    pub album: Option<ReleaseId>,
    pub artist: Option<ArtistId>,
    pub folder: Option<PathBuf>,
    pub available_only: bool,
    pub sort: Sort,
    pub offset: usize,
    pub limit: usize,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ItemId {
    Album(ReleaseId),
    Artist(ArtistId),
    Song(MediaFileId),
    Folder(LibraryRootId, PathBuf),
}
#[derive(Debug, Clone)]
pub struct CollectionItem {
    pub id: ItemId,
    pub title: String,
    pub subtitle: String,
    pub detail: String,
    pub available: bool,
    pub artwork_file: Option<MediaFileId>,
}
#[derive(Debug, Clone)]
pub struct IndexedFile {
    pub file: MediaFile,
    pub projected: bool,
    pub metadata_error: bool,
}

impl Storage {
    pub fn indexed_files(&self, root: LibraryRootId) -> Result<Vec<IndexedFile>, StorageError> {
        self.read(move |tx| {
            let mut stmt = tx.prepare(
                "SELECT f.id, f.relative_path, f.os_volume_id, f.os_file_id,
                f.size_bytes, f.modified_unix_nanos, f.content_digest, f.presence,
                s.file_id IS NOT NULL AND s.needs_refresh=0, s.metadata_error IS NOT NULL
                FROM media_files f LEFT JOIN local_songs s ON s.file_id=f.id WHERE f.root_id=?1",
            )?;
            let mut rows = stmt.query([root.to_bytes()])?;
            let mut files = Vec::new();
            while let Some(row) = rows.next()? {
                let os_file_id = match row.get::<_, Option<String>>(2)? {
                    Some(volume) => Some(OsFileId {
                        volume: VolumeId(volume),
                        file_id: u128::from_be_bytes(blob16(row, 3, "media_files.os_file_id")?),
                    }),
                    None => None,
                };
                files.push(IndexedFile {
                    file: MediaFile {
                        id: MediaFileId::from_bytes(blob16(row, 0, "media_files.id")?),
                        location: FileLocation {
                            root,
                            relative_path: relative_path_from_sql(&row.get::<_, String>(1)?),
                        },
                        os_file_id,
                        signature: signature_from_sql(row.get(4)?, row.get(5)?, row.get(6)?)?,
                        presence: if row.get::<_, String>(7)? == "present" {
                            Presence::Present
                        } else {
                            Presence::Missing
                        },
                    },
                    projected: row.get(8)?,
                    metadata_error: row.get(9)?,
                });
            }
            Ok(files)
        })
    }

    pub fn media_files_with_identity(
        &self,
        identity: OsFileId,
    ) -> Result<Vec<MediaFile>, StorageError> {
        let ids = self.read(move |tx| {
            let mut statement =
                tx.prepare("SELECT id FROM media_files WHERE os_volume_id=?1 AND os_file_id=?2")?;
            let mut rows =
                statement.query(params![identity.volume.0, identity.file_id.to_be_bytes()])?;
            let mut ids = Vec::new();
            while let Some(row) = rows.next()? {
                ids.push(MediaFileId::from_bytes(blob16(row, 0, "media_files.id")?));
            }
            Ok(ids)
        })?;
        ids.into_iter()
            .filter_map(|id| self.media_file(id).transpose())
            .collect()
    }

    pub fn save_song_metadata(
        &self,
        file: MediaFileId,
        meta: SongMetadata,
    ) -> Result<ReleaseId, StorageError> {
        let folder = if meta.folder.as_os_str().is_empty() {
            String::new()
        } else {
            relative_path_to_sql(&meta.folder)?
        };
        self.write(move |tx| {
            let existing: Option<(Vec<u8>, String, bool)> = tx.query_row(
                "SELECT album_id, auto_group_key, manual_group FROM local_songs WHERE file_id=?1",
                [file.to_bytes()], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).optional()?;
            let auto_album = {
                let album: Option<Vec<u8>> = tx.query_row("SELECT id FROM local_albums WHERE group_key=?1", [&meta.group_key], |r| r.get(0)).optional()?;
                match album {
                    Some(id) => id,
                    None => {
                        let id = ReleaseId::generate().to_bytes();
                        tx.execute("INSERT INTO local_albums(id, group_key, title, artist, edition) VALUES(?1,?2,?3,?4,?5)",
                            params![id, meta.group_key, meta.album, meta.album_artist, meta.edition])?;
                        id.to_vec()
                    }
                }
            };
            tx.execute("UPDATE local_albums SET title=?2,artist=?3,edition=?4 WHERE group_key=?1",params![meta.group_key,meta.album,meta.album_artist,meta.edition])?;
            let album_bytes = if let Some((id, _, true)) = &existing { id.clone() } else { auto_album };
            tx.execute("INSERT INTO local_songs(file_id, album_id, auto_group_key, title, artist, folder, disc, track, duration_ms, format, metadata_error)
                VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)
                ON CONFLICT(file_id) DO UPDATE SET album_id=excluded.album_id, auto_group_key=excluded.auto_group_key,
                needs_refresh=0, title=excluded.title, artist=excluded.artist, folder=excluded.folder, disc=excluded.disc,
                track=excluded.track, duration_ms=excluded.duration_ms, format=excluded.format, metadata_error=excluded.metadata_error",
                params![file.to_bytes(), album_bytes, meta.group_key, meta.title, meta.artists.join("; "), folder, meta.disc, meta.track,
                    i64::try_from(meta.duration_ms).unwrap_or(i64::MAX), meta.format, meta.error])?;
            tx.execute("DELETE FROM local_song_artists WHERE file_id=?1", [file.to_bytes()])?;
            for artist in meta.artists {
                tx.execute("INSERT INTO local_artists(id,name) VALUES(?1,?2) ON CONFLICT(name) DO NOTHING", params![ArtistId::generate().to_bytes(), artist])?;
                tx.execute("INSERT OR IGNORE INTO local_song_artists(file_id,artist_id) SELECT ?1,id FROM local_artists WHERE name=?2", params![file.to_bytes(), artist])?;
            }
            Ok(ReleaseId::from_bytes(album_bytes.try_into().map_err(|_| StorageError::InvalidStoredValue { column: "local_albums.id" })?))
        })
    }

    /// A confirmed override changes membership without rewriting tags or physical identities.
    pub fn group_local_files(
        &self,
        files: Vec<MediaFileId>,
        album: ReleaseId,
    ) -> Result<(), StorageError> {
        self.write(move |tx| {
            for file in files {
                let changed = tx.execute(
                    "UPDATE local_songs SET album_id=?2,manual_group=1 WHERE file_id=?1",
                    params![file.to_bytes(), album.to_bytes()],
                )?;
                expect_one(changed, file)?;
            }
            Ok(())
        })
    }

    pub fn reset_local_grouping(&self, files: Vec<MediaFileId>) -> Result<(), StorageError> {
        self.write(move |tx| {
            for file in files {
                let changed = tx.execute("UPDATE local_songs SET manual_group=0,album_id=(SELECT id FROM local_albums WHERE group_key=auto_group_key) WHERE file_id=?1", [file.to_bytes()])?;
                expect_one(changed,file)?;
            }
            Ok(())
        })
    }

    /// Preserve the release ID when its whole automatic folder group moves.
    pub fn move_local_group(&self, old_key: String, new_key: String) -> Result<(), StorageError> {
        self.write(move |tx| {
            tx.execute("UPDATE local_albums SET group_key=?2 WHERE group_key=?1 AND NOT EXISTS(SELECT 1 FROM local_albums WHERE group_key=?2)", params![old_key,new_key])?;
            tx.execute("UPDATE local_songs SET auto_group_key=?2 WHERE auto_group_key=?1", params![old_key,new_key])?;
            Ok(())
        })
    }

    pub fn split_local_group(&self, files: Vec<MediaFileId>) -> Result<ReleaseId, StorageError> {
        let first = files.first().copied().ok_or_else(|| {
            StorageError::Conflict("Select songs to create an album group".into())
        })?;
        self.write(move |tx| {
            let id = ReleaseId::generate();
            let changed = tx.execute("INSERT INTO local_albums(id,group_key,title,artist,edition)
                SELECT ?1,?2,a.title,a.artist,a.edition FROM local_albums a JOIN local_songs s ON s.album_id=a.id WHERE s.file_id=?3",
                params![id.to_bytes(),format!("manual:{id}"),first.to_bytes()])?;
            expect_one(changed,first)?;
            for file in files {
                let changed = tx.execute("UPDATE local_songs SET album_id=?2,manual_group=1 WHERE file_id=?1",params![file.to_bytes(),id.to_bytes()])?;
                expect_one(changed,file)?;
            }
            Ok(id)
        })
    }

    /// Absorb only a newly indexed duplicate that has acquired no user-owned references.
    pub fn recover_media_location(
        &self,
        file: MediaFileId,
        location: FileLocation,
    ) -> Result<(), StorageError> {
        let relative = relative_path_to_sql(&location.relative_path)?;
        self.write(move |tx| {
            let duplicate: Option<Vec<u8>> = tx.query_row("SELECT id FROM media_files WHERE root_id=?1 AND relative_path=?2 AND id<>?3",
                params![location.root.to_bytes(),relative,file.to_bytes()],|r|r.get(0)).optional()?;
            if let Some(duplicate) = duplicate {
                let references: i64 = tx.query_row("SELECT
                    (SELECT COUNT(*) FROM queue_entries WHERE source_kind='file' AND source_id=?1) +
                    (SELECT COUNT(*) FROM track_sources WHERE file_id=?1) +
                    (SELECT COUNT(*) FROM virtual_tracks WHERE image_file_id=?1) +
                    (SELECT COUNT(*) FROM job_items WHERE file_id=?1) +
                    (SELECT COUNT(*) FROM local_songs WHERE file_id=?1 AND manual_group=1)", [&duplicate], |r|r.get(0))?;
                if references > 0 { return Err(StorageError::Conflict("The selected file already has queue, grouping or analysis references. Resolve those references before recovering this file.".into())); }
                tx.execute("DELETE FROM media_files WHERE id=?1",[duplicate])?;
            }
            let changed = tx.execute("UPDATE media_files SET root_id=?2,relative_path=?3,presence='present' WHERE id=?1",params![file.to_bytes(),location.root.to_bytes(),relative])?;
            expect_one(changed,file)?;
            tx.execute("UPDATE local_songs SET manual_group=1 WHERE file_id=?1",[file.to_bytes()])?;
            crate::library::index_for_search(tx,file)
        })
    }

    pub fn local_group_key(&self, file: MediaFileId) -> Result<Option<String>, StorageError> {
        self.read(move |tx| {
            Ok(tx
                .query_row(
                    "SELECT auto_group_key FROM local_songs WHERE file_id=?1",
                    [file.to_bytes()],
                    |r| r.get(0),
                )
                .optional()?)
        })
    }

    pub fn relocate_library_root(
        &self,
        root: LibraryRootId,
        path: &Path,
        volume: Option<VolumeId>,
    ) -> Result<(), StorageError> {
        let path = path_to_sql(path)?.to_owned();
        self.write(move |tx| {
            let changed = tx.execute(
                "UPDATE library_roots SET path=?2,volume_id=?3,availability='online' WHERE id=?1",
                params![root.to_bytes(), path, volume.map(|v| v.0)],
            )?;
            expect_one(changed, root)
        })
    }

    pub fn reconcile_missing_files(&self, files: Vec<MediaFileId>) -> Result<(), StorageError> {
        self.write(move |tx| {
            for file in files {
                tx.execute(
                    "UPDATE media_files SET presence='missing' WHERE id=?1",
                    [file.to_bytes()],
                )?;
            }
            Ok(())
        })
    }

    pub fn resolve_local_file(&self, file: MediaFileId) -> Result<PathBuf, StorageError> {
        self.read(move |tx| {
            let (root,relative): (String,String) = tx.query_row("SELECT r.path,f.relative_path FROM media_files f JOIN library_roots r ON r.id=f.root_id
                WHERE f.id=?1 AND f.presence='present' AND r.availability='online'", [file.to_bytes()], |r| Ok((r.get(0)?,r.get(1)?)))
                .optional()?.ok_or_else(|| StorageError::NotFound(file.to_string()))?;
            Ok(PathBuf::from(root).join(relative_path_from_sql(&relative)))
        })
    }

    pub fn browse_collection(
        &self,
        kind: BrowseKind,
        query: BrowseQuery,
    ) -> Result<Vec<CollectionItem>, StorageError> {
        let mut conditions = vec!["1=1".to_owned()];
        let mut values = Vec::<Value>::new();
        if let Some(root) = query.root {
            conditions.push("f.root_id=?".into());
            values.push(Value::Blob(root.to_bytes().to_vec()));
        }
        if let Some(album) = query.album {
            conditions.push("s.album_id=?".into());
            values.push(Value::Blob(album.to_bytes().to_vec()));
        }
        if let Some(artist) = query.artist {
            conditions.push("EXISTS(SELECT 1 FROM local_song_artists la WHERE la.file_id=f.id AND la.artist_id=?)".into());
            values.push(Value::Blob(artist.to_bytes().to_vec()));
        }
        if let Some(folder) = query.folder {
            conditions.push("s.folder=?".into());
            values.push(Value::Text(if folder.as_os_str().is_empty() {
                String::new()
            } else {
                relative_path_to_sql(&folder)?
            }));
        }
        if query.available_only {
            conditions.push(
                "f.presence='present' AND r.availability='online' AND s.metadata_error IS NULL"
                    .into(),
            );
        }
        let mut phrases = Vec::new();
        for word in query
            .text
            .split_whitespace()
            .map(crate::library::fold_for_search)
        {
            if word.chars().count() >= 3 {
                phrases.push(format!("\"{}\"", word.replace('"', "\"\"")));
            } else {
                conditions.push(
                    "f.seq IN(SELECT rowid FROM media_file_search WHERE instr(folded,?)>0)".into(),
                );
                values.push(Value::Text(word));
            }
        }
        if !phrases.is_empty() {
            conditions.push(
                "f.seq IN(SELECT rowid FROM media_file_search WHERE media_file_search MATCH ?)"
                    .into(),
            );
            values.push(Value::Text(phrases.join(" AND ")));
        }
        let available =
            "f.presence='present' AND r.availability='online' AND s.metadata_error IS NULL";
        let (select, group) = match kind {
            BrowseKind::Albums => (
                format!(
                    "a.id,a.title,a.artist,a.edition,MAX({available}),MIN(CASE WHEN {available} THEN f.id END),NULL"
                ),
                "a.id",
            ),
            BrowseKind::Artists => (
                format!(
                    "ar.id,ar.name,CAST(COUNT(DISTINCT f.id) AS TEXT),'songs',MAX({available}),MIN(CASE WHEN {available} THEN f.id END),NULL"
                ),
                "ar.id",
            ),
            BrowseKind::Songs => (
                format!(
                    "f.id,s.title,s.artist,CASE WHEN s.metadata_error IS NOT NULL THEN s.metadata_error ELSE printf('%d.%02d · %d:%02d · %s',s.disc,s.track,s.duration_ms/60000,(s.duration_ms/1000)%60,s.format) END,{available},CASE WHEN {available} THEN f.id END,NULL"
                ),
                "f.id",
            ),
            BrowseKind::Folders => (
                format!(
                    "r.id,CASE WHEN s.folder='' THEN r.path ELSE s.folder END,r.path,CAST(COUNT(*) AS TEXT),MAX({available}),MIN(CASE WHEN {available} THEN f.id END),s.folder"
                ),
                "r.id,s.folder",
            ),
        };
        let artist_join = if kind == BrowseKind::Artists {
            "JOIN local_song_artists sa ON sa.file_id=f.id JOIN local_artists ar ON ar.id=sa.artist_id"
        } else {
            ""
        };
        let order = match query.sort {
            Sort::RecentlyAdded => "MAX(f.indexed_at_ms) DESC,1",
            Sort::Artist if kind != BrowseKind::Artists => "3 COLLATE NOCASE,2 COLLATE NOCASE,1",
            Sort::TrackOrder if kind == BrowseKind::Songs => {
                "s.disc,s.track,s.title COLLATE NOCASE,f.id"
            }
            _ => "2 COLLATE NOCASE,1",
        };
        let sql = format!(
            "SELECT {select} FROM local_songs s JOIN media_files f ON f.id=s.file_id JOIN library_roots r ON r.id=f.root_id JOIN local_albums a ON a.id=s.album_id {artist_join} WHERE {} GROUP BY {group} ORDER BY {order} LIMIT ? OFFSET ?",
            conditions.join(" AND ")
        );
        values.push(Value::Integer(query.limit.clamp(1, 256) as i64));
        values.push(Value::Integer(
            i64::try_from(query.offset).unwrap_or(i64::MAX),
        ));
        self.read(move |tx| {
            let mut stmt = tx.prepare(&sql)?;
            let mut rows = stmt.query(params_from_iter(values))?;
            let mut items = Vec::new();
            while let Some(row) = rows.next()? {
                let bytes = blob16(row, 0, "collection.id")?;
                let id = match kind {
                    BrowseKind::Albums => ItemId::Album(ReleaseId::from_bytes(bytes)),
                    BrowseKind::Artists => ItemId::Artist(ArtistId::from_bytes(bytes)),
                    BrowseKind::Songs => ItemId::Song(MediaFileId::from_bytes(bytes)),
                    BrowseKind::Folders => ItemId::Folder(
                        LibraryRootId::from_bytes(bytes),
                        relative_path_from_sql(&row.get::<_, String>(6)?),
                    ),
                };
                let art: Option<Vec<u8>> = row.get(5)?;
                items.push(CollectionItem {
                    id,
                    title: row.get(1)?,
                    subtitle: row.get(2)?,
                    detail: row.get(3)?,
                    available: row.get(4)?,
                    artwork_file: art
                        .and_then(|v| v.try_into().ok())
                        .map(MediaFileId::from_bytes),
                });
            }
            Ok(items)
        })
    }
}
