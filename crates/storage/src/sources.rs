//! Provider item identities and links between logical tracks and playable sources.

use parabolica_domain::ids::TrackId;
use parabolica_domain::provider::ProviderItemRef;
use parabolica_domain::source::PlayableSource;
use rusqlite::{Transaction, params};

use crate::codec::{
    SourceColumns, blob16, now_ms, optional_blob16, source_from_sql, source_to_sql,
};
use crate::{Storage, StorageError};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkState {
    /// The user confirmed that the source plays this recording.
    Confirmed,
    /// A reversible proposal with evidence, shown for confirmation.
    Suggested,
    /// The user rejected the proposal; it is kept so it is not suggested again.
    Rejected,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceLink {
    pub track: TrackId,
    pub source: PlayableSource,
    pub state: LinkState,
    /// Why the link was proposed, for example matching duration and ISRC.
    pub evidence: Option<String>,
}

impl Storage {
    /// Record that a provider item exists. Repeated calls are no-ops.
    pub fn record_provider_item(&self, item: ProviderItemRef) -> Result<(), StorageError> {
        if item.item.0.is_empty() {
            return Err(StorageError::invalid("provider item", "has an empty ID"));
        }
        self.write(move |tx| insert_provider_item(tx, item.provider.key(), &item.item.0))
    }

    pub fn create_track(&self) -> Result<TrackId, StorageError> {
        self.write(|tx| {
            let id = TrackId::generate();
            tx.execute(
                "INSERT INTO tracks (id, created_at_ms) VALUES (?1, ?2)",
                params![id.to_bytes(), now_ms()],
            )?;
            Ok(id)
        })
    }

    /// Link `source` to `track`, or update the state and evidence of an existing link between them.
    ///
    /// A source plays one recording: linking a source that is confirmed or suggested for another
    /// track fails with [`StorageError::Conflict`]. A local source must already be recorded; a
    /// provider item is recorded on first use.
    pub fn link_source(
        &self,
        track: TrackId,
        source: PlayableSource,
        state: LinkState,
        evidence: Option<String>,
    ) -> Result<(), StorageError> {
        self.write(move |tx| {
            let columns = source_to_sql(&source)?;
            let (file_id, virtual_track_id) = local_columns(&columns);
            if let (Some(provider), Some(item)) = (columns.provider, columns.provider_item) {
                insert_provider_item(tx, provider, item)?;
            }
            let other_track: Option<Vec<u8>> = tx
                .prepare(
                    "SELECT track_id FROM track_sources
                     WHERE source_kind = ?1 AND file_id IS ?2 AND virtual_track_id IS ?3
                       AND provider IS ?4 AND provider_item_id IS ?5
                       AND link_state <> 'rejected' AND track_id <> ?6",
                )?
                .query_map(
                    params![
                        columns.kind,
                        file_id,
                        virtual_track_id,
                        columns.provider,
                        columns.provider_item,
                        track.to_bytes(),
                    ],
                    |row| row.get(0),
                )?
                .next()
                .transpose()?;
            if other_track.is_some() && state != LinkState::Rejected {
                return Err(StorageError::Conflict(
                    "the source is already linked to another track".to_owned(),
                ));
            }
            let updated = tx.execute(
                "UPDATE track_sources SET link_state = ?7, evidence = ?8, linked_at_ms = ?9
                 WHERE track_id = ?1 AND source_kind = ?2 AND file_id IS ?3
                   AND virtual_track_id IS ?4 AND provider IS ?5 AND provider_item_id IS ?6",
                params![
                    track.to_bytes(),
                    columns.kind,
                    file_id,
                    virtual_track_id,
                    columns.provider,
                    columns.provider_item,
                    link_state_to_sql(state),
                    evidence,
                    now_ms(),
                ],
            )?;
            if updated == 0 {
                tx.execute(
                    "INSERT INTO track_sources (
                         track_id, source_kind, file_id, virtual_track_id, provider,
                         provider_item_id, link_state, evidence, linked_at_ms)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                    params![
                        track.to_bytes(),
                        columns.kind,
                        file_id,
                        virtual_track_id,
                        columns.provider,
                        columns.provider_item,
                        link_state_to_sql(state),
                        evidence,
                        now_ms(),
                    ],
                )?;
            }
            Ok(())
        })
    }

    /// Remove every link of `source`, including rejections.
    pub fn unlink_source(&self, source: PlayableSource) -> Result<(), StorageError> {
        self.write(move |tx| {
            let columns = source_to_sql(&source)?;
            let (file_id, virtual_track_id) = local_columns(&columns);
            tx.execute(
                "DELETE FROM track_sources
                 WHERE source_kind = ?1 AND file_id IS ?2 AND virtual_track_id IS ?3
                   AND provider IS ?4 AND provider_item_id IS ?5",
                params![
                    columns.kind,
                    file_id,
                    virtual_track_id,
                    columns.provider,
                    columns.provider_item
                ],
            )?;
            Ok(())
        })
    }

    /// The confirmed or suggested link of `source`, if any.
    pub fn source_link(&self, source: &PlayableSource) -> Result<Option<SourceLink>, StorageError> {
        let source = source.clone();
        self.read(move |tx| {
            let columns = source_to_sql(&source)?;
            let (file_id, virtual_track_id) = local_columns(&columns);
            let links = query_links(
                tx,
                "WHERE source_kind = ?1 AND file_id IS ?2 AND virtual_track_id IS ?3
                   AND provider IS ?4 AND provider_item_id IS ?5 AND link_state <> 'rejected'",
                params![
                    columns.kind,
                    file_id,
                    virtual_track_id,
                    columns.provider,
                    columns.provider_item
                ],
            )?;
            Ok(links.into_iter().next())
        })
    }

    /// Every link of `track`, including rejected suggestions.
    pub fn track_sources(&self, track: TrackId) -> Result<Vec<SourceLink>, StorageError> {
        self.read(move |tx| query_links(tx, "WHERE track_id = ?1", params![track.to_bytes()]))
    }
}

fn insert_provider_item(
    tx: &Transaction<'_>,
    provider: &str,
    item: &str,
) -> Result<(), StorageError> {
    tx.execute(
        "INSERT INTO provider_items (provider, item_id, first_seen_at_ms) VALUES (?1, ?2, ?3)
         ON CONFLICT DO NOTHING",
        params![provider, item, now_ms()],
    )?;
    Ok(())
}

fn local_columns(columns: &SourceColumns<'_>) -> (Option<[u8; 16]>, Option<[u8; 16]>) {
    match columns.kind {
        "file" => (columns.local_id, None),
        "cue" => (None, columns.local_id),
        _ => (None, None),
    }
}

fn query_links(
    tx: &Transaction<'_>,
    filter: &str,
    params: &[&dyn rusqlite::ToSql],
) -> Result<Vec<SourceLink>, StorageError> {
    let mut statement = tx.prepare(&format!(
        "SELECT track_id, source_kind, file_id, virtual_track_id, provider, provider_item_id,
                link_state, evidence
         FROM track_sources {filter} ORDER BY linked_at_ms"
    ))?;
    let mut rows = statement.query(params)?;
    let mut links = Vec::new();
    while let Some(row) = rows.next()? {
        let kind: String = row.get(1)?;
        let local_id = optional_blob16(row, 2, "track_sources.file_id")?.or(optional_blob16(
            row,
            3,
            "track_sources.virtual_track_id",
        )?);
        links.push(SourceLink {
            track: TrackId::from_bytes(blob16(row, 0, "track_sources.track_id")?),
            source: source_from_sql(&kind, local_id, row.get(4)?, row.get(5)?)?,
            state: link_state_from_sql(&row.get::<_, String>(6)?)?,
            evidence: row.get(7)?,
        });
    }
    Ok(links)
}

fn link_state_to_sql(state: LinkState) -> &'static str {
    match state {
        LinkState::Confirmed => "confirmed",
        LinkState::Suggested => "suggested",
        LinkState::Rejected => "rejected",
    }
}

fn link_state_from_sql(text: &str) -> Result<LinkState, StorageError> {
    match text {
        "confirmed" => Ok(LinkState::Confirmed),
        "suggested" => Ok(LinkState::Suggested),
        "rejected" => Ok(LinkState::Rejected),
        _ => Err(StorageError::InvalidStoredValue {
            column: "track_sources.link_state",
        }),
    }
}
