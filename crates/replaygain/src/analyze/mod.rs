//! BS.1770 measurement with ReplayGain 2.0's -18 LUFS reference.

mod fingerprint;
#[cfg(test)]
mod tests;

use ebur128::{Channel, EbuR128, Mode};
pub use fingerprint::fingerprint;
use parabolica_domain::gain::PeakMethod;
pub use parabolica_domain::gain::analysis::{ANALYZER, AlbumAnalysis, TARGET_LUFS, TrackAnalysis};

#[derive(Debug, thiserror::Error)]
pub enum AnalysisError {
    #[error("Unsupported sample rate or channel layout")]
    UnsupportedFormat,
    #[error("Audio contains non-finite samples or an incomplete PCM frame")]
    InvalidSamples,
    #[error("The decoded file contains no audio frames")]
    Empty,
    #[error("Loudness measurement failed: {0}")]
    Measurement(#[from] ebur128::Error),
}

#[derive(Debug)]
pub struct Analyzer {
    state: EbuR128,
    mask: u64,
    method: PeakMethod,
    step: usize,
    until_boundary: usize,
    frames: u64,
    energies: Vec<f64>,
}

impl Analyzer {
    /// `mask` uses the WAVEFORMATEXTENSIBLE/Symphonia positioned-channel order.
    pub fn new(rate: u32, mask: u64, method: PeakMethod) -> Result<Self, AnalysisError> {
        let mono = mask.count_ones() == 1;
        if !(8000..=768000).contains(&rate) || mask == 0 || (!mono && mask & !0x63f != 0) {
            return Err(AnalysisError::UnsupportedFormat);
        }
        let channels = if mono {
            vec![Channel::DualMono]
        } else {
            (0..11)
                .filter(|bit| mask & (1 << bit) != 0)
                .map(|bit| match bit {
                    0 => Channel::Left,
                    1 => Channel::Right,
                    2 => Channel::Center,
                    3 => Channel::Unused,
                    4 | 9 => Channel::LeftSurround,
                    5 | 10 => Channel::RightSurround,
                    _ => unreachable!(),
                })
                .collect::<Vec<_>>()
        };
        let mode = Mode::M
            | match method {
                PeakMethod::Sample => Mode::SAMPLE_PEAK,
                PeakMethod::TruePeak => Mode::TRUE_PEAK,
            };
        let mut state = EbuR128::new(channels.len() as u32, rate, mode)?;
        state.set_channel_map(&channels)?;
        let step = (rate as usize + 5) / 10;
        Ok(Self {
            state,
            mask,
            method,
            step,
            until_boundary: step * 4,
            frames: 0,
            energies: Vec::new(),
        })
    }

    pub fn add(&mut self, pcm: &[f64]) -> Result<(), AnalysisError> {
        let channels = self.state.channels() as usize;
        if !pcm.len().is_multiple_of(channels) || pcm.iter().any(|s| !s.is_finite()) {
            return Err(AnalysisError::InvalidSamples);
        }
        let mut remaining = pcm;
        while !remaining.is_empty() {
            let frames = self.until_boundary.min(remaining.len() / channels);
            let (block, rest) = remaining.split_at(frames * channels);
            self.state.add_frames_f64(block)?;
            self.frames += frames as u64;
            self.until_boundary -= frames;
            remaining = rest;
            if self.until_boundary == 0 {
                let lufs = self.state.loudness_momentary()?;
                let energy = 10.0_f64.powf((lufs + 0.691) / 10.0);
                if !energy.is_finite() {
                    return Err(AnalysisError::InvalidSamples);
                }
                self.energies.push(energy);
                self.until_boundary = self.step;
            }
        }
        Ok(())
    }

    pub fn finish(mut self) -> Result<TrackAnalysis, AnalysisError> {
        if self.frames == 0 {
            return Err(AnalysisError::Empty);
        }
        let sample_peak = (0..self.state.channels())
            .map(|c| self.state.sample_peak(c))
            .try_fold(0.0_f64, |p, v| Ok::<_, AnalysisError>(p.max(v?)))?;
        let peak = match self.method {
            PeakMethod::Sample => sample_peak,
            PeakMethod::TruePeak => {
                // Drain interpolation delay; these zeros never enter the gating distribution.
                self.state
                    .add_frames_f64(&vec![0.0; 192 * self.state.channels() as usize])?;
                (0..self.state.channels())
                    .map(|c| self.state.true_peak(c))
                    .try_fold(sample_peak, |p, v| Ok::<_, AnalysisError>(p.max(v?)))?
            }
        };
        let loudness_lufs = integrated(self.energies.iter().copied());
        let result = TrackAnalysis {
            sample_rate: self.state.rate(),
            channel_mask: self.mask,
            frames: self.frames,
            peak_method: self.method,
            sample_peak,
            peak,
            energies: self.energies,
            loudness_lufs,
        };
        if !result.valid() {
            return Err(AnalysisError::InvalidSamples);
        }
        Ok(result)
    }
}

/// Combine energies before the relative gate. Tracks retain independent filters and block
/// boundaries, including when members have different rates or layouts.
pub fn integrated(energies: impl IntoIterator<Item = f64>) -> Option<f64> {
    let absolute = 10.0_f64.powf((-70.0 + 0.691) / 10.0);
    let blocks = energies
        .into_iter()
        .filter(|e| *e >= absolute)
        .collect::<Vec<_>>();
    if blocks.is_empty() || blocks.iter().any(|e| !e.is_finite()) {
        return None;
    }
    let threshold = (blocks.iter().sum::<f64>() / blocks.len() as f64 * 0.1).max(absolute);
    let mut count = 0_u64;
    let sum = blocks
        .into_iter()
        .filter(|e| *e >= threshold)
        .inspect(|_| count += 1)
        .sum::<f64>();
    (count > 0).then(|| -0.691 + 10.0 * (sum / count as f64).log10())
}

pub fn album(tracks: &[TrackAnalysis]) -> Option<AlbumAnalysis> {
    if tracks.is_empty()
        || tracks
            .iter()
            .any(|t| !t.valid() || t.peak_method != tracks[0].peak_method)
    {
        return None;
    }
    let loudness_lufs = integrated(tracks.iter().flat_map(|t| t.energies.iter().copied()));
    let peak = tracks.iter().fold(0.0_f64, |p, t| p.max(t.peak));
    Some(AlbumAnalysis {
        loudness_lufs,
        peak,
    })
}
