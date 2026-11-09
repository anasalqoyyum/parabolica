//! Durable measurement data shared by the analyzer and its storage repository.

use super::PeakMethod;

pub const ANALYZER: &str =
    "parabolica-rg2/1;ebur128/0.1.10;bs1770-4;400ms/100ms;absolute=-70;relative=-10";
pub const TARGET_LUFS: f64 = -18.0;

#[derive(Debug, Clone, PartialEq)]
pub struct AlbumAnalysis {
    pub loudness_lufs: Option<f64>,
    pub peak: f64,
}

impl AlbumAnalysis {
    pub fn gain_db(&self) -> Option<f64> {
        self.loudness_lufs.map(|lufs| TARGET_LUFS - lufs)
    }

    pub fn valid(&self) -> bool {
        self.loudness_lufs.is_none_or(f64::is_finite) && self.peak.is_finite() && self.peak >= 0.0
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct TrackAnalysis {
    pub sample_rate: u32,
    pub channel_mask: u64,
    pub frames: u64,
    pub peak_method: PeakMethod,
    pub sample_peak: f64,
    pub peak: f64,
    /// Linear K-weighted energies for complete 400 ms blocks, at 100 ms steps.
    /// Keeping the distribution allows album gating after restart, without decoding again.
    pub energies: Vec<f64>,
    /// None for silence, material below the absolute gate, or audio shorter than one block.
    pub loudness_lufs: Option<f64>,
}

impl TrackAnalysis {
    pub fn gain_db(&self) -> Option<f64> {
        self.loudness_lufs.map(|lufs| TARGET_LUFS - lufs)
    }

    pub fn valid(&self) -> bool {
        (8000..=768000).contains(&self.sample_rate)
            && self.channel_mask != 0
            && self.channel_mask & !0x63f == 0
            && self.frames > 0
            && self.sample_peak.is_finite()
            && self.sample_peak >= 0.0
            && self.peak.is_finite()
            && self.peak >= self.sample_peak
            && (self.peak_method != PeakMethod::Sample || self.peak == self.sample_peak)
            && self.energies.len() as u64
                == (self.frames / ((self.sample_rate as u64 + 5) / 10)).saturating_sub(3)
            && self.loudness_lufs.is_none_or(f64::is_finite)
            && self.energies.iter().all(|e| e.is_finite() && *e >= 0.0)
    }
}
