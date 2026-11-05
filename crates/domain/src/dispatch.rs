//! Batching events before they reach the UI thread.
//!
//! Workers send events over a channel. The UI drains the channel once per frame, coalesces the
//! batch and applies it in one update. Events that only report the latest value of something
//! (position, job progress, snapshots) coalesce by key; everything else is kept in order.

use std::collections::HashSet;
use std::hash::Hash;

pub trait Coalesce {
    type Key: Eq + Hash;

    /// `Some(key)` when a later event with the same key makes this one obsolete.
    fn coalesce_key(&self) -> Option<Self::Key>;
}

/// Drop every keyed event that a later event with the same key replaces. Kept events retain
/// their relative order.
pub fn coalesce<E: Coalesce>(events: Vec<E>) -> Vec<E> {
    let mut seen = HashSet::new();
    let mut kept: Vec<E> = events
        .into_iter()
        .rev()
        .filter(|event| event.coalesce_key().is_none_or(|key| seen.insert(key)))
        .collect();
    kept.reverse();
    kept
}
