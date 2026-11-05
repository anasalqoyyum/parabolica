//! Generations, cancellation and snapshot versions.
//!
//! Rules:
//! - A controller bumps its [`GenerationCounter`] whenever a new request supersedes earlier ones
//!   (a new track, seek or search). Asynchronous work carries the generation it started under in a
//!   [`Tagged`] result; the controller discards results whose generation is no longer current.
//! - Long-running work polls a [`CancelToken`] between bounded units of work and stops without
//!   committing partial results as complete.
//! - Every snapshot a controller publishes carries a strictly increasing [`SnapshotVersion`]. The
//!   UI applies a snapshot only if its version is newer than the last one it applied.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Generation(u64);

impl Generation {
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// Shared source of generations for one controller.
#[derive(Debug, Default)]
pub struct GenerationCounter(AtomicU64);

impl GenerationCounter {
    pub fn current(&self) -> Generation {
        Generation(self.0.load(Ordering::Acquire))
    }

    /// Start a new generation, superseding all earlier ones.
    pub fn advance(&self) -> Generation {
        Generation(self.0.fetch_add(1, Ordering::AcqRel) + 1)
    }

    pub fn is_current(&self, generation: Generation) -> bool {
        self.current() == generation
    }
}

/// A result produced under a specific generation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tagged<T> {
    pub generation: Generation,
    pub value: T,
}

impl<T> Tagged<T> {
    /// The value if it is still current, or `None` for a superseded result.
    pub fn accept(self, counter: &GenerationCounter) -> Option<T> {
        counter.is_current(self.generation).then_some(self.value)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("operation cancelled")]
pub struct Cancelled;

/// Cooperative cancellation shared between a requester and a worker.
#[derive(Debug, Clone, Default)]
pub struct CancelToken(Arc<AtomicBool>);

impl CancelToken {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.0.store(true, Ordering::Release);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }

    /// For use with `?` between units of work.
    pub fn check(&self) -> Result<(), Cancelled> {
        if self.is_cancelled() {
            Err(Cancelled)
        } else {
            Ok(())
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SnapshotVersion(u64);

/// Issues snapshot versions for one publisher.
#[derive(Debug, Default)]
pub struct SnapshotSequencer {
    last: u64,
}

impl SnapshotSequencer {
    pub fn wrap<T>(&mut self, state: T) -> Versioned<T> {
        self.last += 1;
        Versioned {
            version: SnapshotVersion(self.last),
            state,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Versioned<T> {
    pub version: SnapshotVersion,
    pub state: T,
}

/// Receiver-side check that drops stale or duplicate snapshots.
#[derive(Debug, Default)]
pub struct SnapshotGate {
    applied: Option<SnapshotVersion>,
}

impl SnapshotGate {
    pub fn accept<T>(&mut self, snapshot: Versioned<T>) -> Option<T> {
        if self
            .applied
            .is_some_and(|applied| snapshot.version <= applied)
        {
            return None;
        }
        self.applied = Some(snapshot.version);
        Some(snapshot.state)
    }
}
