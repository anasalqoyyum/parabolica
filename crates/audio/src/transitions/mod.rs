//! Deterministic local playback order. Queue identity and shuffle policy belong to the caller.

use parabolica_domain::ids::QueueEntryId;
use parabolica_domain::queue::RepeatMode;
use parabolica_domain::source::ResolvedSource;

#[derive(Debug, Clone)]
pub struct PlaybackSequence {
    pub(crate) entries: Vec<(QueueEntryId, ResolvedSource)>,
    pub(crate) repeat: RepeatMode,
    detached: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SequenceError {
    #[error("the playback sequence is empty")]
    Empty,
    #[error("queue entry {0} occurs twice in the playback sequence")]
    Duplicate(QueueEntryId),
    #[error("queue entry {0} is not in the playback sequence")]
    Missing(QueueEntryId),
}

impl PlaybackSequence {
    pub fn new(
        entries: Vec<(QueueEntryId, ResolvedSource)>,
        repeat: RepeatMode,
    ) -> Result<Self, SequenceError> {
        if entries.is_empty() {
            return Err(SequenceError::Empty);
        }
        let mut seen = std::collections::HashSet::with_capacity(entries.len());
        for (id, _) in &entries {
            if !seen.insert(*id) {
                return Err(SequenceError::Duplicate(*id));
            }
        }
        Ok(Self {
            entries,
            repeat,
            detached: false,
        })
    }

    pub fn entries(&self) -> &[(QueueEntryId, ResolvedSource)] {
        &self.entries
    }

    pub fn repeat(&self) -> RepeatMode {
        self.repeat
    }

    pub(crate) fn prepend_detached(&mut self, entry: QueueEntryId, source: ResolvedSource) {
        self.entries.insert(0, (entry, source));
        self.detached = true;
    }

    pub(crate) fn index(&self, entry: QueueEntryId) -> Result<usize, SequenceError> {
        self.entries
            .iter()
            .position(|(id, _)| *id == entry)
            .ok_or(SequenceError::Missing(entry))
    }

    pub(crate) fn next(&self, index: usize) -> Option<usize> {
        if self.detached && index == 0 {
            return Some(1);
        }
        match self.repeat {
            RepeatMode::One => Some(index),
            _ if index + 1 < self.entries.len() => Some(index + 1),
            RepeatMode::All => Some(usize::from(self.detached)),
            _ => None,
        }
    }
}
