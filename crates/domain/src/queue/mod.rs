//! Queue entries and order with stable entry identities.
//!
//! Each insertion creates new [`QueueEntryId`]s, so the same source queued twice is two entries.
//! Reordering and removal never reassign IDs. Shuffle, repeat, history and persistence build on
//! this order.

use crate::ids::QueueEntryId;
use crate::source::PlayableSource;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct QueueEntry {
    pub id: QueueEntryId,
    pub source: PlayableSource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RepeatMode {
    #[default]
    Off,
    All,
    One,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InsertPosition {
    /// Immediately after the current entry, or at the start when nothing is current.
    AfterCurrent,
    End,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QueueCommand {
    PlayNow(Vec<PlayableSource>),
    Insert {
        sources: Vec<PlayableSource>,
        at: InsertPosition,
    },
    Move {
        entry: QueueEntryId,
        to_index: usize,
    },
    Remove(QueueEntryId),
    Clear,
    Jump(QueueEntryId),
    SetRepeat(RepeatMode),
    SetShuffle {
        seed: Option<u64>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum QueueError {
    #[error("{0} is not in the queue")]
    UnknownEntry(QueueEntryId),
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct QueueOrder {
    entries: Vec<QueueEntry>,
    current: Option<QueueEntryId>,
}

impl QueueOrder {
    pub fn entries(&self) -> &[QueueEntry] {
        &self.entries
    }

    pub fn current(&self) -> Option<QueueEntryId> {
        self.current
    }

    pub fn index_of(&self, entry: QueueEntryId) -> Option<usize> {
        self.entries
            .iter()
            .position(|candidate| candidate.id == entry)
    }

    pub fn insert(
        &mut self,
        sources: impl IntoIterator<Item = PlayableSource>,
        at: InsertPosition,
    ) -> Vec<QueueEntryId> {
        let index = match at {
            InsertPosition::End => self.entries.len(),
            InsertPosition::AfterCurrent => self
                .current
                .and_then(|current| self.index_of(current))
                .map_or(0, |index| index + 1),
        };
        let new_entries: Vec<QueueEntry> = sources
            .into_iter()
            .map(|source| QueueEntry {
                id: QueueEntryId::generate(),
                source,
            })
            .collect();
        let ids = new_entries.iter().map(|entry| entry.id).collect();
        self.entries.splice(index..index, new_entries);
        ids
    }

    /// Move an entry so it ends up at `to_index` (clamped to the end).
    pub fn move_entry(&mut self, entry: QueueEntryId, to_index: usize) -> Result<(), QueueError> {
        let from = self
            .index_of(entry)
            .ok_or(QueueError::UnknownEntry(entry))?;
        let moved = self.entries.remove(from);
        let to_index = to_index.min(self.entries.len());
        self.entries.insert(to_index, moved);
        Ok(())
    }

    /// Remove an entry. Removing the current entry leaves nothing current.
    pub fn remove(&mut self, entry: QueueEntryId) -> Result<QueueEntry, QueueError> {
        let index = self
            .index_of(entry)
            .ok_or(QueueError::UnknownEntry(entry))?;
        if self.current == Some(entry) {
            self.current = None;
        }
        Ok(self.entries.remove(index))
    }

    pub fn set_current(&mut self, entry: QueueEntryId) -> Result<(), QueueError> {
        self.index_of(entry)
            .ok_or(QueueError::UnknownEntry(entry))?;
        self.current = Some(entry);
        Ok(())
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.current = None;
    }
}
