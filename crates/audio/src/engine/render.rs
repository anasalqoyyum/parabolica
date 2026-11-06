//! The real-time side of a stream: consumes prepared frames from the ring buffer.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

use parabolica_domain::format::PcmSample;
use rtrb::Consumer;

use crate::realtime::{RealtimeRender, RenderOutcome, UnderrunCounter};

/// State shared between the controller, the decoder worker and the renderer. Every field is an
/// atomic so the renderer never waits.
///
/// Flushing uses epochs. The worker stops writing, stores a new `epoch_request` and waits. The
/// renderer discards everything readable, resets `played` and stores the epoch in `epoch_ack`.
/// Only then does the worker write frames for the new epoch, so a stale frame can never follow a
/// seek or a new track.
#[derive(Debug, Default)]
pub(crate) struct StreamShared {
    pub playing: AtomicBool,
    /// Target app volume as `f32` bits, linear in `[0, 1]`.
    pub volume: AtomicU32,
    pub epoch_request: AtomicU64,
    pub epoch_ack: AtomicU64,
    /// Frames rendered since the acknowledged epoch began.
    pub played: AtomicU64,
    /// Epoch whose last frame the worker has written; `u64::MAX` for none.
    pub source_end: AtomicU64,
    /// Epoch whose last frame the renderer has played; `u64::MAX` for none.
    pub drained: AtomicU64,
    pub underruns: UnderrunCounter,
}

impl StreamShared {
    pub fn new(volume: f32) -> Self {
        Self {
            volume: AtomicU32::new(volume.to_bits()),
            source_end: AtomicU64::new(u64::MAX),
            drained: AtomicU64::new(u64::MAX),
            ..Self::default()
        }
    }

    /// Frames played in `epoch`, or 0 while the renderer has not reached it.
    pub fn played_in(&self, epoch: u64) -> u64 {
        if self.epoch_ack.load(Ordering::Acquire) == epoch {
            self.played.load(Ordering::Acquire)
        } else {
            0
        }
    }
}

/// A device sample type the renderer can write.
pub trait OutputSample: Copy + Send + 'static {
    const SILENCE: Self;
    fn from_pcm(sample: PcmSample) -> Self;
}

impl OutputSample for f32 {
    const SILENCE: Self = 0.0;

    fn from_pcm(sample: PcmSample) -> Self {
        sample as f32
    }
}

impl OutputSample for i16 {
    const SILENCE: Self = 0;

    fn from_pcm(sample: PcmSample) -> Self {
        (sample * 32_768.0).round().clamp(-32_768.0, 32_767.0) as i16
    }
}

impl OutputSample for i32 {
    const SILENCE: Self = 0;

    fn from_pcm(sample: PcmSample) -> Self {
        (sample * 2_147_483_648.0)
            .round()
            .clamp(-2_147_483_648.0, 2_147_483_647.0) as i32
    }
}

/// Time for the gain to move by full scale on a volume change, pause or resume. A new track or a
/// seek starts at the target gain without a fade.
const RAMP_SECONDS: f64 = 0.01;

/// Renders one output stream. Created by the engine and handed to the backend, which calls
/// [`RealtimeRender::render`] from its device thread.
#[derive(Debug)]
pub struct StreamRenderer {
    consumer: Consumer<PcmSample>,
    shared: Arc<StreamShared>,
    channels: usize,
    epoch: u64,
    epoch_started: bool,
    gain: f64,
    ramp_step: f64,
}

impl StreamRenderer {
    pub(crate) fn new(
        consumer: Consumer<PcmSample>,
        shared: Arc<StreamShared>,
        channels: usize,
        rate: u32,
    ) -> Self {
        Self {
            consumer,
            shared,
            channels,
            epoch: 0,
            epoch_started: false,
            gain: 0.0,
            ramp_step: 1.0 / (RAMP_SECONDS * f64::from(rate)),
        }
    }

    pub fn channels(&self) -> usize {
        self.channels
    }

    #[cfg(test)]
    pub(crate) fn shared(&self) -> &Arc<StreamShared> {
        &self.shared
    }

    fn apply_flush(&mut self) {
        let request = self.shared.epoch_request.load(Ordering::Acquire);
        if request == self.epoch {
            return;
        }
        let pending = self.consumer.slots();
        if let Ok(chunk) = self.consumer.read_chunk(pending) {
            chunk.commit_all();
        }
        self.shared.played.store(0, Ordering::Release);
        self.epoch = request;
        self.epoch_started = false;
        // A fade left over from the old epoch must not consume the new epoch's first frames;
        // only `playing` starts it, at the target gain.
        self.gain = 0.0;
        self.shared.epoch_ack.store(request, Ordering::Release);
    }

    /// Frames to consume this block while fading out after a pause.
    fn fade_out_frames(&self) -> usize {
        (self.gain / self.ramp_step).ceil() as usize
    }
}

impl<S: OutputSample> RealtimeRender<S> for StreamRenderer {
    fn render(&mut self, out: &mut [S], channels: usize) -> RenderOutcome {
        debug_assert_eq!(channels, self.channels);
        self.apply_flush();

        let playing = self.shared.playing.load(Ordering::Acquire);
        let volume = f64::from(f32::from_bits(self.shared.volume.load(Ordering::Relaxed)));
        let target = if playing { volume } else { 0.0 };
        if playing && !self.epoch_started {
            self.gain = target;
        }

        let frames = out.len() / channels;
        let available = self.consumer.slots() / channels;
        let wanted = if playing {
            frames
        } else {
            frames.min(self.fade_out_frames())
        };
        let count = wanted.min(available);

        if count > 0 {
            let chunk = self
                .consumer
                .read_chunk(count * channels)
                .expect("slots were checked");
            let (first, second) = chunk.as_slices();
            let mut source = first.iter().chain(second);
            for frame in out[..count * channels].chunks_exact_mut(channels) {
                if self.gain < target {
                    self.gain = (self.gain + self.ramp_step).min(target);
                } else if self.gain > target {
                    self.gain = (self.gain - self.ramp_step).max(target);
                }
                for sample in frame {
                    let value = *source.next().expect("chunk holds whole frames");
                    *sample = S::from_pcm(value * self.gain);
                }
            }
            chunk.commit_all();
            self.epoch_started = true;
            self.shared.played.fetch_add(count as u64, Ordering::AcqRel);
        }
        out[count * channels..].fill(S::SILENCE);

        if !playing {
            // Snap a finished fade to silence, including one cut short by an empty buffer.
            if count < wanted || self.gain < self.ramp_step * 1e-3 {
                self.gain = 0.0;
            }
            return RenderOutcome::Filled;
        }
        if count == frames {
            return RenderOutcome::Filled;
        }
        // Read the end marker before the buffer: the worker publishes it after its last write.
        if self.shared.source_end.load(Ordering::Acquire) == self.epoch && self.consumer.is_empty()
        {
            self.shared.drained.store(self.epoch, Ordering::Release);
            return RenderOutcome::Finished;
        }
        self.shared.underruns.record();
        RenderOutcome::Underrun {
            missing_frames: frames - count,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    // The crate's test binary registers the counting allocator in `realtime::tests`.
    use crate::realtime::probe::allocations_during;

    fn renderer(
        capacity_frames: usize,
    ) -> (rtrb::Producer<f64>, StreamRenderer, Arc<StreamShared>) {
        let (producer, consumer) = rtrb::RingBuffer::new(capacity_frames * 2);
        let shared = Arc::new(StreamShared::new(1.0));
        let renderer = StreamRenderer::new(consumer, shared.clone(), 2, 1000);
        (producer, renderer, shared)
    }

    #[test]
    fn renders_exact_samples_at_unity_and_counts_underruns_without_allocating() {
        let (mut producer, mut renderer, shared) = renderer(16);
        let samples: Vec<f64> = (0..12).map(|i| f64::from(i) / 16.0).collect();
        producer.push_entire_slice(&samples).unwrap();
        shared.playing.store(true, Ordering::Release);
        let mut block = vec![1.0f32; 8];

        let (outcomes, allocations) = allocations_during(|| {
            [
                renderer.render(&mut block[..], 2),
                renderer.render(&mut block[..], 2),
            ]
        });

        assert_eq!(allocations, 0);
        assert_eq!(
            outcomes,
            [
                RenderOutcome::Filled,
                RenderOutcome::Underrun { missing_frames: 2 }
            ]
        );
        assert_eq!(block, [0.5, 0.5625, 0.625, 0.6875, 0.0, 0.0, 0.0, 0.0]);
        assert_eq!(shared.played.load(Ordering::Acquire), 6);
        assert_eq!(shared.underruns.total(), 1);
    }

    #[test]
    fn flush_discards_old_frames_and_acknowledges() {
        let (mut producer, mut renderer, shared) = renderer(16);
        producer.push_entire_slice(&[0.5; 8]).unwrap();
        shared.playing.store(true, Ordering::Release);
        shared.epoch_request.store(7, Ordering::Release);
        let mut block = [1.0f32; 4];
        let (_, allocations) = allocations_during(|| renderer.render(&mut block[..], 2));
        assert_eq!(allocations, 0);
        assert_eq!(block, [0.0; 4]);
        assert_eq!(shared.epoch_ack.load(Ordering::Acquire), 7);
        assert_eq!(shared.played_in(7), 0);
        assert_eq!(shared.played_in(6), 0);
    }

    #[test]
    fn a_flush_ends_any_fade_from_the_old_epoch() {
        let (mut producer, mut renderer, shared) = renderer(16);
        producer.push_entire_slice(&[0.5; 8]).unwrap();
        shared.playing.store(true, Ordering::Release);
        let mut block = [0.0f32; 4];
        renderer.render(&mut block[..], 2);
        assert_eq!(renderer.gain, 1.0);

        shared.playing.store(false, Ordering::Release);
        shared.epoch_request.store(3, Ordering::Release);
        renderer.apply_flush();
        assert_eq!(renderer.gain, 0.0);
        // New-epoch frames written right after the acknowledgement stay queued while paused.
        producer.push_entire_slice(&[0.25; 8]).unwrap();
        renderer.render(&mut block[..], 2);
        assert_eq!(block, [0.0; 4]);
        assert_eq!(shared.played_in(3), 0);
    }

    #[test]
    fn reports_finished_once_the_source_is_drained() {
        let (mut producer, mut renderer, shared) = renderer(16);
        producer.push_entire_slice(&[0.25; 4]).unwrap();
        shared.playing.store(true, Ordering::Release);
        shared.source_end.store(0, Ordering::Release);
        let mut block = [0.0f32; 8];
        assert_eq!(renderer.render(&mut block[..], 2), RenderOutcome::Finished);
        assert_eq!(block, [0.25, 0.25, 0.25, 0.25, 0.0, 0.0, 0.0, 0.0]);
        assert_eq!(shared.underruns.total(), 0);
    }

    #[test]
    fn pause_fades_out_then_stops_consuming() {
        let (mut producer, mut renderer, shared) = renderer(64);
        producer.push_entire_slice(&[1.0; 128]).unwrap();
        shared.playing.store(true, Ordering::Release);
        let mut block = [0.0f32; 8];
        renderer.render(&mut block[..], 2);
        assert_eq!(block, [1.0; 8]);

        shared.playing.store(false, Ordering::Release);
        let mut block = [0.0f32; 40];
        assert_eq!(renderer.render(&mut block[..], 2), RenderOutcome::Filled);
        // A 10 ms ramp at 1 kHz is 10 frames; the first frame already steps down.
        let left: Vec<f32> = block.iter().step_by(2).copied().collect();
        assert!(left[..10].windows(2).all(|pair| pair[0] > pair[1]));
        assert!(left[9].abs() < 1e-9 && left[10..].iter().all(|v| *v == 0.0));
        let played = shared.played.load(Ordering::Acquire);
        assert_eq!(played, 4 + 10);

        renderer.render(&mut block[..], 2);
        assert_eq!(shared.played.load(Ordering::Acquire), played);
        assert_eq!(shared.underruns.total(), 0);
    }

    #[test]
    fn volume_change_ramps_and_integer_output_rounds() {
        let (mut producer, mut renderer, shared) = renderer(64);
        producer.push_entire_slice(&[0.5; 64]).unwrap();
        shared.playing.store(true, Ordering::Release);
        let mut block = [0i16; 4];
        renderer.render(&mut block[..], 2);
        assert_eq!(block, [16_384; 4]);

        shared.volume.store(0.5f32.to_bits(), Ordering::Release);
        let mut block = [0i16; 48];
        renderer.render(&mut block[..], 2);
        let left: Vec<i16> = block.iter().step_by(2).copied().collect();
        assert!(left[..5].windows(2).all(|pair| pair[0] > pair[1]));
        assert!(left[4..].iter().all(|value| *value == 8_192), "{left:?}");
    }
}
