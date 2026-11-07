use std::collections::HashSet;

use parabolica_domain::ids::{PlaylistEntryId, PlaylistId};
use parabolica_domain::source::PlayableSource;
use rusqlite::{Transaction, params};

use crate::codec::{blob16, now_ms, optional_blob16, source_from_sql, source_to_sql};
use crate::library::expect_one;
use crate::{Storage, StorageError};

mod files;
pub mod m3u8;
pub use files::SourceDetails;

pub const PLAYLIST_ENTRY_LIMIT: usize = 10_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaylistEntry {
    pub id: PlaylistEntryId,
    pub source: PlayableSource,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Playlist {
    pub id: PlaylistId,
    pub name: String,
    pub entries: Vec<PlaylistEntry>,
}

impl Storage {
    pub fn create_playlist(&self, name: &str) -> Result<PlaylistId, StorageError> {
        let name = playlist_name(name)?;
        self.write(move |tx| create_in(tx, &name))
    }

    pub fn rename_playlist(&self, playlist: PlaylistId, name: &str) -> Result<(), StorageError> {
        let name = playlist_name(name)?;
        self.write(move |tx| {
            expect_one(
                tx.execute(
                    "UPDATE playlists SET name = ?2 WHERE id = ?1",
                    params![playlist.to_bytes(), name],
                )?,
                playlist,
            )
        })
    }

    pub fn delete_playlist(&self, playlist: PlaylistId) -> Result<(), StorageError> {
        self.write(move |tx| {
            expect_one(
                tx.execute("DELETE FROM playlists WHERE id = ?1", [playlist.to_bytes()])?,
                playlist,
            )
        })
    }

    pub fn playlists(&self) -> Result<Vec<Playlist>, StorageError> {
        self.read(|tx| {
            let mut statement =
                tx.prepare("SELECT id, name FROM playlists ORDER BY created_at_ms, id")?;
            let mut rows = statement.query([])?;
            let mut playlists = Vec::new();
            while let Some(row) = rows.next()? {
                let id = PlaylistId::from_bytes(blob16(row, 0, "playlists.id")?);
                playlists.push(Playlist {
                    id,
                    name: row.get(1)?,
                    entries: entries_in(tx, id)?,
                });
            }
            Ok(playlists)
        })
    }

    pub fn playlist(&self, id: PlaylistId) -> Result<Playlist, StorageError> {
        self.read(move |tx| {
            let name = tx.query_row(
                "SELECT name FROM playlists WHERE id = ?1",
                [id.to_bytes()],
                |row| row.get(0),
            )?;
            Ok(Playlist {
                id,
                name,
                entries: entries_in(tx, id)?,
            })
        })
    }

    pub fn append_playlist(
        &self,
        playlist: PlaylistId,
        sources: Vec<PlayableSource>,
    ) -> Result<Vec<PlaylistEntryId>, StorageError> {
        self.write(move |tx| append_in(tx, playlist, &sources))
    }

    /// Reorder the complete playlist by stable entry IDs; invalid permutations change nothing.
    pub fn reorder_playlist(
        &self,
        playlist: PlaylistId,
        order: Vec<PlaylistEntryId>,
    ) -> Result<(), StorageError> {
        self.write(move |tx| {
            require_playlist(tx, playlist)?;
            let entries = entries_in(tx, playlist)?;
            let ids: HashSet<_> = entries.iter().map(|entry| entry.id).collect();
            if order.len() != ids.len()
                || order.iter().collect::<HashSet<_>>().len() != ids.len()
                || order.iter().any(|id| !ids.contains(id))
            {
                return Err(StorageError::invalid(
                    "playlist order",
                    "is not a permutation of its entries",
                ));
            }
            for (index, entry) in order.iter().enumerate() {
                tx.execute(
                    "UPDATE playlist_entries SET position = ?2 WHERE id = ?1",
                    params![entry.to_bytes(), index as i64],
                )?;
            }
            Ok(())
        })
    }

    pub fn remove_playlist_entry(
        &self,
        playlist: PlaylistId,
        entry: PlaylistEntryId,
    ) -> Result<(), StorageError> {
        self.write(move |tx| {
            expect_one(
                tx.execute(
                    "DELETE FROM playlist_entries WHERE playlist_id = ?1 AND id = ?2",
                    params![playlist.to_bytes(), entry.to_bytes()],
                )?,
                entry,
            )
        })
    }

    /// An app-owned like, including a like of a provider identity. It performs no provider write.
    pub fn set_local_like(&self, source: PlayableSource, liked: bool) -> Result<(), StorageError> {
        self.write(move |tx| {
            let columns = source_to_sql(&source)?;
            let key = source_key(&source);
            if liked {
                tx.execute("INSERT INTO local_likes (source_key, source_kind, source_id, provider, provider_item_id, liked_at_ms)
                    VALUES (?1, ?2, ?3, ?4, ?5, ?6) ON CONFLICT (source_key) DO NOTHING",
                    params![key, columns.kind, columns.local_id, columns.provider, columns.provider_item, now_ms()])?;
            } else {
                tx.execute("DELETE FROM local_likes WHERE source_key = ?1", [key])?;
            }
            Ok(())
        })
    }

    pub fn local_likes(&self) -> Result<Vec<PlayableSource>, StorageError> {
        self.read(|tx| {
            let mut statement = tx.prepare("SELECT source_kind, source_id, provider, provider_item_id FROM local_likes ORDER BY liked_at_ms, source_key")?;
            let mut rows = statement.query([])?;
            let mut sources = Vec::new();
            while let Some(row) = rows.next()? {
                sources.push(source_from_sql(&row.get::<_, String>(0)?, optional_blob16(row, 1, "local_likes.source_id")?, row.get(2)?, row.get(3)?)?);
            }
            Ok(sources)
        })
    }
}

fn source_key(source: &PlayableSource) -> String {
    match source {
        PlayableSource::LocalFile(id) => id.to_string(),
        PlayableSource::CueTrack(id) => id.to_string(),
        PlayableSource::Provider(item) => format!("{}:{}", item.provider.key(), item.item.0),
    }
}

fn playlist_name(name: &str) -> Result<String, StorageError> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 256 || name.contains(['\r', '\n', '\0']) {
        return Err(StorageError::invalid(
            "playlist name",
            "must have 1 to 256 characters without line breaks or NUL",
        ));
    }
    Ok(name.to_owned())
}

fn create_in(tx: &Transaction<'_>, name: &str) -> Result<PlaylistId, StorageError> {
    let id = PlaylistId::generate();
    tx.execute(
        "INSERT INTO playlists (id, name, created_at_ms) VALUES (?1, ?2, ?3)",
        params![id.to_bytes(), name, now_ms()],
    )?;
    Ok(id)
}

fn require_playlist(tx: &Transaction<'_>, playlist: PlaylistId) -> Result<(), StorageError> {
    let exists: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM playlists WHERE id = ?1)",
        [playlist.to_bytes()],
        |row| row.get(0),
    )?;
    if !exists {
        return Err(StorageError::NotFound(playlist.to_string()));
    }
    Ok(())
}

fn entries_in(
    tx: &Transaction<'_>,
    playlist: PlaylistId,
) -> Result<Vec<PlaylistEntry>, StorageError> {
    let mut statement = tx.prepare("SELECT id, source_kind, source_id, provider, provider_item_id FROM playlist_entries WHERE playlist_id = ?1 ORDER BY position, id")?;
    let mut rows = statement.query([playlist.to_bytes()])?;
    let mut entries = Vec::new();
    while let Some(row) = rows.next()? {
        entries.push(PlaylistEntry {
            id: PlaylistEntryId::from_bytes(blob16(row, 0, "playlist_entries.id")?),
            source: source_from_sql(
                &row.get::<_, String>(1)?,
                optional_blob16(row, 2, "playlist_entries.source_id")?,
                row.get(3)?,
                row.get(4)?,
            )?,
        });
    }
    Ok(entries)
}

fn append_in(
    tx: &Transaction<'_>,
    playlist: PlaylistId,
    sources: &[PlayableSource],
) -> Result<Vec<PlaylistEntryId>, StorageError> {
    require_playlist(tx, playlist)?;
    let (count, next): (i64, i64) = tx.query_row("SELECT count(*), coalesce(max(position) + 1, 0) FROM playlist_entries WHERE playlist_id = ?1", [playlist.to_bytes()], |row| Ok((row.get(0)?, row.get(1)?)))?;
    if (count as usize).saturating_add(sources.len()) > PLAYLIST_ENTRY_LIMIT {
        return Err(StorageError::invalid("playlist", "exceeds 10000 entries"));
    }
    let mut ids = Vec::new();
    for (index, source) in sources.iter().enumerate() {
        let id = PlaylistEntryId::generate();
        let columns = source_to_sql(source)?;
        tx.execute("INSERT INTO playlist_entries (id, playlist_id, position, source_kind, source_id, provider, provider_item_id)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)", params![id.to_bytes(), playlist.to_bytes(), next + index as i64,
                columns.kind, columns.local_id, columns.provider, columns.provider_item])?;
        ids.push(id);
    }
    Ok(ids)
}
