use std::collections::HashSet;

use parabolica_domain::ids::QueueEntryId;
use parabolica_domain::source::PlayableSource;
use rusqlite::params;

use crate::codec::{blob16, optional_blob16, source_from_sql, source_to_sql};
use crate::queue::{QueueCheckpoint, load_queue_in, position_to_sql, save_queue_in};
use crate::{Storage, StorageError};

pub const MAX_RETRY_ATTEMPTS: u8 = 3;
pub const HISTORY_LIMIT: usize = 1000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnavailableEntry {
    pub entry: QueueEntryId,
    pub reason: String,
    pub retry_attempts: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListeningRecord {
    pub entry: QueueEntryId,
    pub source: PlayableSource,
    pub started_at_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionCheckpoint {
    pub queue: QueueCheckpoint,
    pub navigation: Vec<QueueEntryId>,
    pub unavailable: Vec<UnavailableEntry>,
    /// The latest 1000 confirmed starts, oldest first. Queue removal never removes these records.
    pub history: Vec<ListeningRecord>,
}

impl Storage {
    pub fn save_session(&self, session: SessionCheckpoint) -> Result<(), StorageError> {
        session.queue.validate()?;
        let ids: HashSet<_> = session.queue.entries.iter().map(|entry| entry.id).collect();
        let mut unavailable = HashSet::new();
        if session.navigation.iter().any(|id| !ids.contains(id))
            || session.unavailable.iter().any(|entry| {
                !ids.contains(&entry.entry)
                    || !unavailable.insert(entry.entry)
                    || entry.retry_attempts > MAX_RETRY_ATTEMPTS
            })
        {
            return Err(StorageError::invalid(
                "session",
                "has invalid navigation or retry state",
            ));
        }
        let position = position_to_sql(session.queue.position)?;
        self.write(move |tx| {
            save_queue_in(tx, &session.queue, position)?;
            for (index, entry) in session.navigation.iter().enumerate() {
                tx.execute("INSERT INTO queue_navigation (position, entry_id) VALUES (?1, ?2)",
                    params![index as i64, entry.to_bytes()])?;
            }
            for entry in session.unavailable {
                tx.execute("INSERT INTO queue_unavailable (entry_id, reason, retry_attempts) VALUES (?1, ?2, ?3)",
                    params![entry.entry.to_bytes(), entry.reason, entry.retry_attempts])?;
            }
            tx.execute("DELETE FROM listening_history", [])?;
            let start = session.history.len().saturating_sub(HISTORY_LIMIT);
            for (index, record) in session.history[start..].iter().enumerate() {
                let source = source_to_sql(&record.source)?;
                tx.execute("INSERT INTO listening_history
                    (position, entry_id, source_kind, source_id, provider, provider_item_id, started_at_ms)
                    VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                    params![index as i64, record.entry.to_bytes(), source.kind, source.local_id,
                        source.provider, source.provider_item, record.started_at_ms])?;
            }
            Ok(())
        })
    }

    pub fn load_session(&self) -> Result<Option<SessionCheckpoint>, StorageError> {
        self.read(|tx| {
            let Some(queue) = load_queue_in(tx)? else {
                return Ok(None);
            };
            let mut navigation = Vec::new();
            let mut statement =
                tx.prepare("SELECT entry_id FROM queue_navigation ORDER BY position")?;
            let mut rows = statement.query([])?;
            while let Some(row) = rows.next()? {
                navigation.push(QueueEntryId::from_bytes(blob16(
                    row,
                    0,
                    "queue_navigation.entry_id",
                )?));
            }
            let mut unavailable = Vec::new();
            let mut statement = tx.prepare(
                "SELECT entry_id, reason, retry_attempts FROM queue_unavailable ORDER BY entry_id",
            )?;
            let mut rows = statement.query([])?;
            while let Some(row) = rows.next()? {
                unavailable.push(UnavailableEntry {
                    entry: QueueEntryId::from_bytes(blob16(row, 0, "queue_unavailable.entry_id")?),
                    reason: row.get(1)?,
                    retry_attempts: row.get(2)?,
                });
            }
            let mut history = Vec::new();
            let mut statement = tx.prepare(
                "SELECT entry_id, source_kind, source_id, provider, provider_item_id, started_at_ms
                FROM listening_history ORDER BY position",
            )?;
            let mut rows = statement.query([])?;
            while let Some(row) = rows.next()? {
                history.push(ListeningRecord {
                    entry: QueueEntryId::from_bytes(blob16(row, 0, "listening_history.entry_id")?),
                    source: source_from_sql(
                        &row.get::<_, String>(1)?,
                        optional_blob16(row, 2, "listening_history.source_id")?,
                        row.get(3)?,
                        row.get(4)?,
                    )?,
                    started_at_ms: row.get(5)?,
                });
            }
            Ok(Some(SessionCheckpoint {
                queue,
                navigation,
                unavailable,
                history,
            }))
        })
    }
}
