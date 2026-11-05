//! The play queue checkpoint: order, current entry, position, repeat and shuffle, saved in one
//! transaction. Restoring yields the most recent complete checkpoint.

use std::collections::{HashMap, HashSet};
use std::time::Duration;

use parabolica_domain::ids::QueueEntryId;
use parabolica_domain::queue::{QueueEntry, RepeatMode};
use rusqlite::{OptionalExtension, params};

use crate::codec::{blob16, now_ms, optional_blob16, source_from_sql, source_to_sql};
use crate::{Storage, StorageError};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueueCheckpoint {
    pub entries: Vec<QueueEntry>,
    pub current: Option<QueueEntryId>,
    /// Position within the current entry.
    pub position: Duration,
    pub repeat: RepeatMode,
    pub shuffle: Option<Shuffle>,
}

/// The shuffled play order: every queue entry exactly once.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shuffle {
    pub seed: u64,
    pub order: Vec<QueueEntryId>,
}

impl QueueCheckpoint {
    fn validate(&self) -> Result<(), StorageError> {
        let mut ids = HashSet::with_capacity(self.entries.len());
        if !self.entries.iter().all(|entry| ids.insert(entry.id)) {
            return Err(StorageError::invalid("queue", "repeats an entry ID"));
        }
        if self.current.is_some_and(|current| !ids.contains(&current)) {
            return Err(StorageError::invalid(
                "queue",
                "current entry is not in the queue",
            ));
        }
        if let Some(shuffle) = &self.shuffle {
            let shuffled: HashSet<_> = shuffle.order.iter().collect();
            if shuffle.order.len() != self.entries.len()
                || shuffled.len() != shuffle.order.len()
                || !shuffle.order.iter().all(|id| ids.contains(id))
            {
                return Err(StorageError::invalid(
                    "queue",
                    "shuffle order is not a permutation of the entries",
                ));
            }
        }
        Ok(())
    }
}

impl Storage {
    /// Replace the saved queue with `checkpoint`.
    pub fn save_queue(&self, checkpoint: QueueCheckpoint) -> Result<(), StorageError> {
        checkpoint.validate()?;
        let position = position_to_sql(checkpoint.position)?;
        self.write(move |tx| {
            tx.execute("DELETE FROM queue_entries", [])?;
            let shuffle_positions: Option<HashMap<QueueEntryId, i64>> =
                checkpoint.shuffle.as_ref().map(|shuffle| {
                    shuffle
                        .order
                        .iter()
                        .enumerate()
                        .map(|(index, id)| (*id, index as i64))
                        .collect()
                });
            let mut insert = tx.prepare(
                "INSERT INTO queue_entries (
                     position, entry_id, source_kind, source_id, provider, provider_item_id,
                     shuffle_position)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            )?;
            for (position, entry) in checkpoint.entries.iter().enumerate() {
                let source = source_to_sql(&entry.source)?;
                let shuffle_position = shuffle_positions
                    .as_ref()
                    .map(|positions| positions[&entry.id]);
                insert.execute(params![
                    position as i64,
                    entry.id.to_bytes(),
                    source.kind,
                    source.local_id,
                    source.provider,
                    source.provider_item,
                    shuffle_position,
                ])?;
            }
            tx.execute(
                "INSERT INTO queue_state (
                     singleton, current_entry_id, position_micros, repeat_mode, shuffle_seed,
                     saved_at_ms)
                 VALUES (1, ?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT (singleton) DO UPDATE SET
                     current_entry_id = excluded.current_entry_id,
                     position_micros = excluded.position_micros,
                     repeat_mode = excluded.repeat_mode,
                     shuffle_seed = excluded.shuffle_seed,
                     saved_at_ms = excluded.saved_at_ms",
                params![
                    checkpoint.current.map(QueueEntryId::to_bytes),
                    position,
                    repeat_to_sql(checkpoint.repeat),
                    checkpoint
                        .shuffle
                        .as_ref()
                        .map(|shuffle| i64::from_ne_bytes(shuffle.seed.to_ne_bytes())),
                    now_ms(),
                ],
            )?;
            Ok(())
        })
    }

    /// Update the saved position if `entry` is still the saved current entry. Returns whether it
    /// was, so a late checkpoint for a previous track changes nothing.
    pub fn save_playback_position(
        &self,
        entry: QueueEntryId,
        position: Duration,
    ) -> Result<bool, StorageError> {
        let position = position_to_sql(position)?;
        self.write(move |tx| {
            let changed = tx.execute(
                "UPDATE queue_state SET position_micros = ?2, saved_at_ms = ?3
                 WHERE singleton = 1 AND current_entry_id = ?1",
                params![entry.to_bytes(), position, now_ms()],
            )?;
            Ok(changed == 1)
        })
    }

    /// The saved queue, or `None` before the first save.
    pub fn load_queue(&self) -> Result<Option<QueueCheckpoint>, StorageError> {
        self.read(|tx| {
            let state = tx
                .query_row(
                    "SELECT current_entry_id, position_micros, repeat_mode, shuffle_seed
                     FROM queue_state WHERE singleton = 1",
                    [],
                    |row| {
                        Ok((
                            row.get::<_, Option<Vec<u8>>>(0)?,
                            row.get::<_, i64>(1)?,
                            row.get::<_, String>(2)?,
                            row.get::<_, Option<i64>>(3)?,
                        ))
                    },
                )
                .optional()?;
            let Some((current, position, repeat, seed)) = state else {
                return Ok(None);
            };
            let mut statement = tx.prepare(
                "SELECT entry_id, source_kind, source_id, provider, provider_item_id, shuffle_position
                 FROM queue_entries ORDER BY position",
            )?;
            let mut rows = statement.query([])?;
            let mut entries = Vec::new();
            let mut shuffled = Vec::new();
            while let Some(row) = rows.next()? {
                let id = QueueEntryId::from_bytes(blob16(row, 0, "queue_entries.entry_id")?);
                let source = source_from_sql(
                    &row.get::<_, String>(1)?,
                    optional_blob16(row, 2, "queue_entries.source_id")?,
                    row.get(3)?,
                    row.get(4)?,
                )?;
                if let Some(shuffle_position) = row.get::<_, Option<i64>>(5)? {
                    shuffled.push((shuffle_position, id));
                }
                entries.push(QueueEntry { id, source });
            }
            shuffled.sort_unstable();
            let current = current
                .map(|bytes| {
                    bytes.try_into().map(QueueEntryId::from_bytes).map_err(|_| {
                        StorageError::InvalidStoredValue {
                            column: "queue_state.current_entry_id",
                        }
                    })
                })
                .transpose()?;
            let checkpoint = QueueCheckpoint {
                entries,
                current,
                position: Duration::from_micros(u64::try_from(position).map_err(|_| {
                    StorageError::InvalidStoredValue {
                        column: "queue_state.position_micros",
                    }
                })?),
                repeat: repeat_from_sql(&repeat)?,
                shuffle: seed.map(|seed| Shuffle {
                    seed: u64::from_ne_bytes(seed.to_ne_bytes()),
                    order: shuffled.into_iter().map(|(_, id)| id).collect(),
                }),
            };
            checkpoint
                .validate()
                .map_err(|_| StorageError::InvalidStoredValue { column: "queue_entries" })?;
            Ok(Some(checkpoint))
        })
    }
}

fn position_to_sql(position: Duration) -> Result<i64, StorageError> {
    i64::try_from(position.as_micros())
        .map_err(|_| StorageError::invalid("playback position", "is too large"))
}

fn repeat_to_sql(repeat: RepeatMode) -> &'static str {
    match repeat {
        RepeatMode::Off => "off",
        RepeatMode::All => "all",
        RepeatMode::One => "one",
    }
}

fn repeat_from_sql(text: &str) -> Result<RepeatMode, StorageError> {
    match text {
        "off" => Ok(RepeatMode::Off),
        "all" => Ok(RepeatMode::All),
        "one" => Ok(RepeatMode::One),
        _ => Err(StorageError::InvalidStoredValue {
            column: "queue_state.repeat_mode",
        }),
    }
}
