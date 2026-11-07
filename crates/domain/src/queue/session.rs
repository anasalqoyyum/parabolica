use std::collections::HashSet;

use super::{InsertPosition, QueueCommand, QueueEntry, QueueError, QueueOrder, RepeatMode};
use crate::ids::QueueEntryId;
use crate::source::PlayableSource;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShuffleOrder {
    pub seed: u64,
    pub order: Vec<QueueEntryId>,
}

/// Queue edits use effective play-order indices while shuffle is enabled. Turning shuffle off
/// returns to the underlying insertion order. Explicit navigation ignores repeat-one.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct QueueSession {
    order: QueueOrder,
    repeat: RepeatMode,
    shuffle: Option<ShuffleOrder>,
    history: Vec<QueueEntryId>,
}

impl QueueSession {
    pub fn restore(
        entries: Vec<QueueEntry>,
        current: Option<QueueEntryId>,
        repeat: RepeatMode,
        shuffle: Option<ShuffleOrder>,
        history: Vec<QueueEntryId>,
    ) -> Result<Self, QueueError> {
        let ids: HashSet<_> = entries.iter().map(|entry| entry.id).collect();
        if ids.len() != entries.len()
            || current.is_some_and(|id| !ids.contains(&id))
            || history.iter().any(|id| !ids.contains(id))
            || shuffle.as_ref().is_some_and(|shuffle| {
                shuffle.order.len() != ids.len()
                    || shuffle.order.iter().collect::<HashSet<_>>().len() != ids.len()
                    || shuffle.order.iter().any(|id| !ids.contains(id))
            })
        {
            return Err(QueueError::InvalidState);
        }
        Ok(Self {
            order: QueueOrder { entries, current },
            repeat,
            shuffle,
            history,
        })
    }

    pub fn entries(&self) -> &[QueueEntry] {
        self.order.entries()
    }

    pub fn current(&self) -> Option<QueueEntryId> {
        self.order.current()
    }

    pub fn repeat(&self) -> RepeatMode {
        self.repeat
    }

    pub fn shuffle(&self) -> Option<&ShuffleOrder> {
        self.shuffle.as_ref()
    }

    /// The navigation stack contains entries that actually started, rather than queued sources.
    pub fn history(&self) -> &[QueueEntryId] {
        &self.history
    }

    pub fn effective_order(&self) -> Vec<QueueEntryId> {
        self.shuffle.as_ref().map_or_else(
            || self.entries().iter().map(|entry| entry.id).collect(),
            |shuffle| shuffle.order.clone(),
        )
    }

    pub fn insert(
        &mut self,
        sources: impl IntoIterator<Item = PlayableSource>,
        at: InsertPosition,
    ) -> Vec<QueueEntryId> {
        let entries = sources
            .into_iter()
            .map(|source| QueueEntry {
                id: QueueEntryId::generate(),
                source,
            })
            .collect();
        self.insert_entries(entries, at)
            .expect("generated entry IDs are unique")
    }

    pub fn insert_entries(
        &mut self,
        entries: Vec<QueueEntry>,
        at: InsertPosition,
    ) -> Result<Vec<QueueEntryId>, QueueError> {
        let mut seen: HashSet<_> = self.entries().iter().map(|entry| entry.id).collect();
        if entries.iter().any(|entry| !seen.insert(entry.id)) {
            return Err(QueueError::InvalidState);
        }
        let current = self.current();
        let index = match at {
            InsertPosition::End => self.entries().len(),
            InsertPosition::AfterCurrent => current
                .and_then(|id| self.order.index_of(id))
                .map_or(0, |index| index + 1),
        };
        let ids: Vec<_> = entries.iter().map(|entry| entry.id).collect();
        self.order.entries.splice(index..index, entries);
        if let Some(shuffle) = &mut self.shuffle {
            let index = match at {
                InsertPosition::End => shuffle.order.len(),
                InsertPosition::AfterCurrent => current
                    .and_then(|id| shuffle.order.iter().position(|item| *item == id))
                    .map_or(0, |index| index + 1),
            };
            shuffle.order.splice(index..index, ids.iter().copied());
        }
        Ok(ids)
    }

    /// Apply a queue edit. Play now inserts before the next item and selects the first new entry.
    /// Removing the selected entry selects its effective successor, or stops at the end.
    pub fn apply(&mut self, command: QueueCommand) -> Result<Vec<QueueEntryId>, QueueError> {
        match command {
            QueueCommand::PlayNow(sources) => {
                let ids = self.insert(sources, InsertPosition::AfterCurrent);
                if let Some(id) = ids.first() {
                    self.order.set_current(*id)?;
                }
                return Ok(ids);
            }
            QueueCommand::Insert { sources, at } => return Ok(self.insert(sources, at)),
            QueueCommand::Move { entry, to_index } => {
                if let Some(shuffle) = &mut self.shuffle {
                    let from = shuffle
                        .order
                        .iter()
                        .position(|id| *id == entry)
                        .ok_or(QueueError::UnknownEntry(entry))?;
                    let id = shuffle.order.remove(from);
                    shuffle.order.insert(to_index.min(shuffle.order.len()), id);
                } else {
                    self.order.move_entry(entry, to_index)?;
                }
            }
            QueueCommand::Remove(entry) => {
                let successor = if self.current() == Some(entry) {
                    let order = self.effective_order();
                    let index = order
                        .iter()
                        .position(|id| *id == entry)
                        .ok_or(QueueError::UnknownEntry(entry))?;
                    order.get(index + 1).copied()
                } else {
                    self.current()
                };
                self.order.remove(entry)?;
                self.order.current = successor;
                if let Some(shuffle) = &mut self.shuffle {
                    shuffle.order.retain(|id| *id != entry);
                }
                self.history.retain(|id| *id != entry);
            }
            QueueCommand::Clear => {
                self.order.clear();
                self.history.clear();
                if let Some(shuffle) = &mut self.shuffle {
                    shuffle.order.clear();
                }
            }
            QueueCommand::Jump(entry) => self.order.set_current(entry)?,
            QueueCommand::SetRepeat(repeat) => self.repeat = repeat,
            QueueCommand::SetShuffle { seed } => {
                self.shuffle = seed.map(|seed| {
                    let mut order: Vec<_> = self.entries().iter().map(|entry| entry.id).collect();
                    let mut state = seed;
                    for index in (1..order.len()).rev() {
                        let random = splitmix64(&mut state);
                        order.swap(index, (random % (index as u64 + 1)) as usize);
                    }
                    if let Some(current) = self.current()
                        && let Some(index) = order.iter().position(|id| *id == current)
                    {
                        order.rotate_left(index);
                    }
                    ShuffleOrder { seed, order }
                });
            }
        }
        Ok(Vec::new())
    }

    /// Called only for a confirmed engine selection. Repeat boundaries are retained by the
    /// listening ledger; consecutive identical entries add nothing to previous navigation.
    pub fn record_started(&mut self, id: QueueEntryId) -> Result<(), QueueError> {
        self.order.set_current(id)?;
        if self.history.last() != Some(&id) {
            self.history.push(id);
            if self.history.len() > 1000 {
                self.history.remove(0);
            }
        }
        Ok(())
    }

    pub fn stop(&mut self) {
        self.order.current = None;
    }

    pub fn next(&mut self, automatic: bool) -> Option<QueueEntryId> {
        let order = self.effective_order();
        let selected = match self
            .current()
            .and_then(|id| order.iter().position(|item| *item == id))
        {
            Some(index) if automatic && self.repeat == RepeatMode::One => order.get(index),
            Some(index) => order.get(index + 1).or_else(|| {
                (self.repeat == RepeatMode::All)
                    .then(|| order.first())
                    .flatten()
            }),
            None => order.first(),
        }
        .copied();
        self.order.current = selected;
        selected
    }

    /// Previous follows actual listening history. Without history it uses the preceding item
    /// in effective order. It stops at the first item unless repeat-all is enabled.
    pub fn previous(&mut self) -> Option<QueueEntryId> {
        if self.history.last().copied() == self.current() {
            self.history.pop();
        }
        let previous = self.history.pop().or_else(|| {
            let order = self.effective_order();
            let index = self
                .current()
                .and_then(|id| order.iter().position(|item| *item == id))?;
            index
                .checked_sub(1)
                .and_then(|index| order.get(index).copied())
                .or_else(|| {
                    (self.repeat == RepeatMode::All)
                        .then(|| order.last().copied())
                        .flatten()
                })
        });
        self.order.current = previous;
        previous
    }
}

fn splitmix64(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9e3779b97f4a7c15);
    let mut value = *state;
    value = (value ^ (value >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94d049bb133111eb);
    value ^ (value >> 31)
}

#[cfg(test)]
mod tests;
