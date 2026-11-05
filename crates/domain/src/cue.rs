//! CUE virtual track ranges over a physical image.
//!
//! Positions are sample frames at the image's sample rate. CUE `MM:SS:FF` timestamps use 75
//! frames per second; they convert exactly for every rate divisible by 75, which includes all
//! 44.1 kHz and 48 kHz families.

use std::ops::Range;

use crate::format::SampleRate;
use crate::ids::{MediaFileId, VirtualTrackId};

const CUE_FRAMES_PER_SECOND: u64 = 75;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CueTimestamp {
    pub minutes: u32,
    pub seconds: u8,
    pub frames: u8,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CueRangeError {
    #[error("CUE timestamp has seconds >= 60 or frames >= 75")]
    InvalidTimestamp,
    #[error("{0} Hz is not divisible by 75, so CUE offsets do not map to whole sample frames")]
    NonIntegralRate(u32),
    #[error("CUE sheet has no tracks")]
    Empty,
    #[error("track {track} starts before the previous track")]
    OutOfOrder { track: u8 },
    #[error("track {track} INDEX 00 is after its INDEX 01")]
    PregapAfterStart { track: u8 },
    #[error("track {track} starts at or beyond the end of the image")]
    BeyondEnd { track: u8 },
}

impl CueTimestamp {
    /// Convert to a sample-frame offset at `rate`.
    pub fn to_frame(self, rate: SampleRate) -> Result<u64, CueRangeError> {
        if self.seconds >= 60 || u64::from(self.frames) >= CUE_FRAMES_PER_SECOND {
            return Err(CueRangeError::InvalidTimestamp);
        }
        let hz = u64::from(rate.hz());
        if hz % CUE_FRAMES_PER_SECOND != 0 {
            return Err(CueRangeError::NonIntegralRate(rate.hz()));
        }
        let cue_frames = (u64::from(self.minutes) * 60 + u64::from(self.seconds))
            * CUE_FRAMES_PER_SECOND
            + u64::from(self.frames);
        Ok(cue_frames * (hz / CUE_FRAMES_PER_SECOND))
    }
}

/// Which track owns the gap between a track's INDEX 00 and INDEX 01.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PregapPolicy {
    /// The gap plays at the end of the previous track (the common player default).
    #[default]
    AppendToPrevious,
    /// The gap plays at the start of its own track.
    PrependToTrack,
}

/// Index points for one CUE track, already converted to frames.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CueTrackIndexes {
    pub number: u8,
    pub pregap_start: Option<u64>,
    pub start: u64,
}

/// A playable frame range of one virtual track inside its physical image.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VirtualTrackRange {
    pub id: VirtualTrackId,
    pub image: MediaFileId,
    pub number: u8,
    pub frames: Range<u64>,
    /// The INDEX 01 position, inside `frames` when the pregap is prepended.
    pub index_01: u64,
}

/// Build contiguous, non-overlapping ranges covering `[first track start, total_frames)`.
///
/// Audio before the first track's playable start (a hidden first-track pregap with
/// [`PregapPolicy::AppendToPrevious`]) is not part of any track. No frame is counted twice, so
/// summing per-track analysis covers the image exactly once.
pub fn virtual_track_ranges(
    image: MediaFileId,
    tracks: &[CueTrackIndexes],
    total_frames: u64,
    policy: PregapPolicy,
    mut new_id: impl FnMut(u8) -> VirtualTrackId,
) -> Result<Vec<VirtualTrackRange>, CueRangeError> {
    if tracks.is_empty() {
        return Err(CueRangeError::Empty);
    }
    let playable_start = |track: &CueTrackIndexes| match policy {
        PregapPolicy::AppendToPrevious => track.start,
        PregapPolicy::PrependToTrack => track.pregap_start.unwrap_or(track.start),
    };

    let mut previous_start = None;
    for track in tracks {
        if track
            .pregap_start
            .is_some_and(|pregap| pregap > track.start)
        {
            return Err(CueRangeError::PregapAfterStart {
                track: track.number,
            });
        }
        if track.start >= total_frames {
            return Err(CueRangeError::BeyondEnd {
                track: track.number,
            });
        }
        let start = playable_start(track);
        if previous_start.is_some_and(|previous| start <= previous) {
            return Err(CueRangeError::OutOfOrder {
                track: track.number,
            });
        }
        previous_start = Some(start);
    }

    Ok(tracks
        .iter()
        .enumerate()
        .map(|(position, track)| {
            let end = tracks
                .get(position + 1)
                .map_or(total_frames, playable_start);
            VirtualTrackRange {
                id: new_id(track.number),
                image,
                number: track.number,
                frames: playable_start(track)..end,
                index_01: track.start,
            }
        })
        .collect())
}
