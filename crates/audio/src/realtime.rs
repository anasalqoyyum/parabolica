//! The real-time audio callback contract.
//!
//! A device callback runs on a thread the OS or driver schedules against a hard deadline. Code
//! reachable from it must not allocate or free memory, take locks, wait on other threads, touch
//! files, the database or the network, or log. It copies prepared frames from a lock-free buffer
//! that decoder workers fill, converts them to the device sample type, writes silence for any
//! shortage and records the underrun in an atomic counter that other threads read.
//!
//! Stream setup, reconfiguration after a driver reset and teardown happen outside the callback.

use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderOutcome {
    Filled,
    /// The buffer ran short; `missing_frames` frames of silence were written.
    Underrun {
        missing_frames: usize,
    },
    /// The stream has no further audio; the rest of the block is silence.
    Finished,
}

/// Implemented by the engine's per-stream renderer and called from the device callback.
///
/// `out` holds interleaved samples of the negotiated output type for `channels` channels.
/// Implementations must fill every sample and must follow the module-level rules.
pub trait RealtimeRender<S>: Send {
    fn render(&mut self, out: &mut [S], channels: usize) -> RenderOutcome;
}

/// Underrun count written by the callback and read elsewhere for diagnostics.
#[derive(Debug, Default)]
pub struct UnderrunCounter(AtomicU64);

impl UnderrunCounter {
    pub fn record(&self) {
        self.0.fetch_add(1, Ordering::Relaxed);
    }

    pub fn total(&self) -> u64 {
        self.0.load(Ordering::Relaxed)
    }
}

/// A counting allocator for tests that assert a renderer does not allocate.
///
/// Register it as the test binary's global allocator, then wrap the callback body in
/// [`probe::allocations_during`]. Enable the `test-support` feature to use it from other crates.
#[cfg(any(test, feature = "test-support"))]
#[allow(unsafe_code)]
pub mod probe {
    use std::alloc::{GlobalAlloc, Layout, System};
    use std::cell::Cell;

    thread_local! {
        static TRACKING: Cell<bool> = const { Cell::new(false) };
        static EVENTS: Cell<usize> = const { Cell::new(0) };
    }

    fn note() {
        let _ = TRACKING.try_with(|tracking| {
            if tracking.get() {
                EVENTS.with(|events| events.set(events.get() + 1));
            }
        });
    }

    #[derive(Debug)]
    pub struct CountingAllocator;

    // SAFETY: every method forwards to `System` with the caller's arguments unchanged.
    unsafe impl GlobalAlloc for CountingAllocator {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            note();
            unsafe { System.alloc(layout) }
        }

        unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
            note();
            unsafe { System.alloc_zeroed(layout) }
        }

        unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
            note();
            unsafe { System.dealloc(ptr, layout) }
        }

        unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
            note();
            unsafe { System.realloc(ptr, layout, new_size) }
        }
    }

    /// Run `f` and count allocations and frees it made on the current thread.
    pub fn allocations_during<R>(f: impl FnOnce() -> R) -> (R, usize) {
        EVENTS.with(|events| events.set(0));
        TRACKING.with(|tracking| tracking.set(true));
        let result = f();
        TRACKING.with(|tracking| tracking.set(false));
        (result, EVENTS.with(Cell::get))
    }
}

#[cfg(test)]
mod tests {
    use super::probe::{CountingAllocator, allocations_during};
    use super::*;

    #[global_allocator]
    static ALLOCATOR: CountingAllocator = CountingAllocator;

    /// Reads from a pre-filled buffer the way the engine's ring-buffer consumer will.
    struct PreparedFrames<'a> {
        frames: Vec<f32>,
        read: usize,
        underruns: &'a UnderrunCounter,
    }

    impl RealtimeRender<f32> for PreparedFrames<'_> {
        fn render(&mut self, out: &mut [f32], _channels: usize) -> RenderOutcome {
            let available = (self.frames.len() - self.read).min(out.len());
            out[..available].copy_from_slice(&self.frames[self.read..self.read + available]);
            out[available..].fill(0.0);
            self.read += available;
            let missing = out.len() - available;
            if missing == 0 {
                RenderOutcome::Filled
            } else {
                self.underruns.record();
                RenderOutcome::Underrun {
                    missing_frames: missing / 2,
                }
            }
        }
    }

    fn assert_send<T: Send>(_: &T) {}

    #[test]
    fn probe_detects_allocation() {
        let ((), events) = allocations_during(|| drop(std::hint::black_box(vec![1u8; 64])));
        assert!(
            events >= 2,
            "allocation and free must both be counted, saw {events}"
        );
    }

    #[test]
    fn render_and_underrun_accounting_do_not_allocate() {
        let underruns = UnderrunCounter::default();
        let mut renderer = PreparedFrames {
            frames: vec![0.25; 6],
            read: 0,
            underruns: &underruns,
        };
        assert_send(&renderer);
        let mut block = vec![1.0f32; 4];

        let (outcomes, events) = allocations_during(|| {
            [
                renderer.render(&mut block, 2),
                renderer.render(&mut block, 2),
            ]
        });

        assert_eq!(events, 0);
        assert_eq!(
            outcomes,
            [
                RenderOutcome::Filled,
                RenderOutcome::Underrun { missing_frames: 1 }
            ]
        );
        assert_eq!(block, [0.25, 0.25, 0.0, 0.0]);
        assert_eq!(underruns.total(), 1);
    }
}
