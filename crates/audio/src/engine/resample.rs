//! The engine's single sample-rate conversion stage.
//!
//! When the source rate differs from the negotiated output rate, the decoder worker converts with
//! rubato's synchronous FFT resampler in `f64`. Nothing else in the path changes the rate: WASAPI
//! Shared is opened at the endpoint's mix format, so Windows does not resample the stream again.
//! The resampler's startup delay is trimmed and its tail flushed, so N input frames become
//! `round(N * to / from)` output frames.

use parabolica_domain::format::PcmSample;
use rubato::audioadapter_buffers::direct::InterleavedSlice;
use rubato::{Fft, FixedSync, Indexing, Resampler};

const CHUNK_FRAMES: usize = 1024;

pub(crate) struct RateConverter {
    resampler: Fft<PcmSample>,
    channels: usize,
    from: u32,
    to: u32,
    input: Vec<PcmSample>,
    output: Vec<PcmSample>,
    delay_left: usize,
    frames_in: u64,
    frames_out: u64,
}

impl std::fmt::Debug for RateConverter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RateConverter")
            .field("from", &self.from)
            .field("to", &self.to)
            .field("channels", &self.channels)
            .finish_non_exhaustive()
    }
}

impl RateConverter {
    pub fn new(from: u32, to: u32, channels: usize) -> Result<Self, String> {
        let resampler = Fft::new(
            from as usize,
            to as usize,
            CHUNK_FRAMES,
            channels,
            FixedSync::Input,
        )
        .map_err(|error| error.to_string())?;
        let output = vec![0.0; resampler.output_frames_max() * channels];
        let delay_left = resampler.output_delay();
        Ok(Self {
            resampler,
            channels,
            from,
            to,
            input: Vec::with_capacity(CHUNK_FRAMES * channels * 2),
            output,
            delay_left,
            frames_in: 0,
            frames_out: 0,
        })
    }

    /// Convert interleaved `input` and append whatever output is ready to `out`.
    pub fn process(&mut self, input: &[PcmSample], out: &mut Vec<PcmSample>) {
        self.frames_in += (input.len() / self.channels) as u64;
        self.input.extend_from_slice(input);
        while self.input.len() / self.channels >= self.resampler.input_frames_next() {
            let needed = self.resampler.input_frames_next();
            let produced = self.run(needed, None);
            self.input.drain(..needed * self.channels);
            self.emit(produced, u64::MAX, out);
        }
    }

    /// Flush the remaining input at the end of a segment.
    pub fn finish(&mut self, out: &mut Vec<PcmSample>) {
        let expected = (u128::from(self.frames_in) * u128::from(self.to)
            + u128::from(self.from) / 2)
            / u128::from(self.from);
        let expected = u64::try_from(expected).unwrap_or(u64::MAX);
        while self.frames_out < expected {
            let needed = self.resampler.input_frames_next();
            let partial = self.input.len() / self.channels;
            self.input.resize(needed * self.channels, 0.0);
            let produced = self.run(needed, Some(partial));
            self.input.clear();
            self.emit(produced, expected, out);
        }
    }

    fn run(&mut self, frames: usize, partial: Option<usize>) -> usize {
        let input = InterleavedSlice::new(&self.input[..], self.channels, frames)
            .expect("input holds the frames requested");
        let capacity = self.output.len() / self.channels;
        let mut output = InterleavedSlice::new_mut(&mut self.output[..], self.channels, capacity)
            .expect("output sized for output_frames_max");
        let indexing = partial.map(|frames| Indexing::new().partial_len(frames));
        let (_, produced) = self
            .resampler
            .process_into_buffer(&input, &mut output, indexing.as_ref())
            .expect("buffers are sized for the resampler");
        produced
    }

    fn emit(&mut self, produced: usize, limit: u64, out: &mut Vec<PcmSample>) {
        let skip = self.delay_left.min(produced);
        self.delay_left -= skip;
        let available = (produced - skip) as u64;
        let take = available.min(limit.saturating_sub(self.frames_out)) as usize;
        let start = skip * self.channels;
        out.extend_from_slice(&self.output[start..start + take * self.channels]);
        self.frames_out += take as u64;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(rate: u32, hz: f64, frames: usize) -> Vec<f64> {
        (0..frames)
            .flat_map(|n| {
                let value =
                    0.5 * (2.0 * std::f64::consts::PI * hz * n as f64 / f64::from(rate)).sin();
                [value, -value]
            })
            .collect()
    }

    #[test]
    fn converts_44k1_to_48k_with_exact_length_and_alignment() {
        let mut converter = RateConverter::new(44_100, 48_000, 2).unwrap();
        let input = sine(44_100, 1000.0, 44_100);
        let mut out = Vec::new();
        for block in input.chunks(4096 * 2) {
            converter.process(block, &mut out);
        }
        converter.finish(&mut out);
        assert_eq!(out.len(), 48_000 * 2);

        let expected = sine(48_000, 1000.0, 48_000);
        // Skip the edges, where the filter sees the implicit silence around the clip.
        let error = out[2000..94_000]
            .iter()
            .zip(&expected[2000..94_000])
            .map(|(a, b)| (a - b).abs())
            .fold(0.0, f64::max);
        assert!(error < 1e-3, "max error {error}");
    }
}
