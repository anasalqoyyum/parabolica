use parabolica_domain::cue::{
    CueRangeError, CueTimestamp, CueTrackIndexes, PregapPolicy, VirtualTrackRange,
    virtual_track_ranges,
};
use parabolica_domain::format::SampleRate;
use parabolica_domain::ids::{MediaFileId, VirtualTrackId};
use parabolica_domain::source::{FrameSpan, PlayableSource, ResolvedSource};

fn rate(hz: u32) -> SampleRate {
    SampleRate::new(hz).unwrap()
}

fn ts(minutes: u32, seconds: u8, frames: u8) -> CueTimestamp {
    CueTimestamp {
        minutes,
        seconds,
        frames,
    }
}

fn ranges(
    tracks: &[CueTrackIndexes],
    total: u64,
    policy: PregapPolicy,
) -> Result<Vec<VirtualTrackRange>, CueRangeError> {
    virtual_track_ranges(MediaFileId::generate(), tracks, total, policy, |_| {
        VirtualTrackId::generate()
    })
}

#[test]
fn cue_timestamps_convert_to_exact_frames_for_common_rates() {
    // 1 CUE frame = 1/75 s = 588 frames at 44.1 kHz.
    assert_eq!(ts(0, 0, 1).to_frame(rate(44_100)), Ok(588));
    assert_eq!(
        ts(3, 25, 74).to_frame(rate(44_100)),
        Ok((205 * 75 + 74) * 588)
    );
    for hz in [44_100, 48_000, 88_200, 96_000, 176_400, 192_000] {
        assert_eq!(ts(1, 0, 0).to_frame(rate(hz)), Ok(u64::from(hz) * 60));
    }
}

#[test]
fn cue_timestamps_reject_invalid_fields_and_inexact_rates() {
    assert_eq!(
        ts(0, 60, 0).to_frame(rate(44_100)),
        Err(CueRangeError::InvalidTimestamp)
    );
    assert_eq!(
        ts(0, 0, 75).to_frame(rate(44_100)),
        Err(CueRangeError::InvalidTimestamp)
    );
    assert_eq!(
        ts(0, 1, 0).to_frame(rate(22_050 + 1)),
        Err(CueRangeError::NonIntegralRate(22_051))
    );
}

fn three_tracks() -> [CueTrackIndexes; 3] {
    [
        CueTrackIndexes {
            number: 1,
            pregap_start: None,
            start: 0,
        },
        CueTrackIndexes {
            number: 2,
            pregap_start: Some(1_000),
            start: 1_200,
        },
        CueTrackIndexes {
            number: 3,
            pregap_start: Some(2_900),
            start: 3_000,
        },
    ]
}

#[test]
fn ranges_cover_the_image_once_without_overlap() {
    for policy in [PregapPolicy::AppendToPrevious, PregapPolicy::PrependToTrack] {
        let ranges = ranges(&three_tracks(), 5_000, policy).unwrap();
        assert_eq!(ranges.first().unwrap().frames.start, 0);
        assert_eq!(ranges.last().unwrap().frames.end, 5_000);
        for pair in ranges.windows(2) {
            assert_eq!(pair[0].frames.end, pair[1].frames.start, "{policy:?}");
        }
        let covered: u64 = ranges
            .iter()
            .map(|range| range.frames.end - range.frames.start)
            .sum();
        assert_eq!(covered, 5_000, "{policy:?}");
    }
}

#[test]
fn pregap_policy_decides_which_track_owns_index_00() {
    let appended = ranges(&three_tracks(), 5_000, PregapPolicy::AppendToPrevious).unwrap();
    assert_eq!(appended[0].frames, 0..1_200);
    assert_eq!(appended[1].frames, 1_200..3_000);

    let prepended = ranges(&three_tracks(), 5_000, PregapPolicy::PrependToTrack).unwrap();
    assert_eq!(prepended[0].frames, 0..1_000);
    assert_eq!(prepended[1].frames, 1_000..2_900);
    assert_eq!(prepended[1].index_01, 1_200);
}

#[test]
fn hidden_first_track_pregap_belongs_to_no_track_by_default() {
    let tracks = [
        CueTrackIndexes {
            number: 1,
            pregap_start: Some(0),
            start: 400,
        },
        CueTrackIndexes {
            number: 2,
            pregap_start: None,
            start: 900,
        },
    ];
    assert_eq!(
        ranges(&tracks, 2_000, PregapPolicy::AppendToPrevious).unwrap()[0].frames,
        400..900
    );
    assert_eq!(
        ranges(&tracks, 2_000, PregapPolicy::PrependToTrack).unwrap()[0].frames,
        0..900
    );
}

#[test]
fn malformed_index_points_are_rejected() {
    let mut out_of_order = three_tracks();
    out_of_order[2].start = 1_100;
    out_of_order[2].pregap_start = None;
    assert_eq!(
        ranges(&out_of_order, 5_000, PregapPolicy::AppendToPrevious),
        Err(CueRangeError::OutOfOrder { track: 3 })
    );

    let mut late_pregap = three_tracks();
    late_pregap[1].pregap_start = Some(1_300);
    assert_eq!(
        ranges(&late_pregap, 5_000, PregapPolicy::AppendToPrevious),
        Err(CueRangeError::PregapAfterStart { track: 2 })
    );

    assert_eq!(
        ranges(&three_tracks(), 3_000, PregapPolicy::AppendToPrevious),
        Err(CueRangeError::BeyondEnd { track: 3 })
    );
    assert_eq!(
        ranges(&[], 10, PregapPolicy::AppendToPrevious),
        Err(CueRangeError::Empty)
    );
}

#[test]
fn cue_track_keeps_virtual_identity_and_resolves_to_its_image_range() {
    let image = MediaFileId::generate();
    let ranges = virtual_track_ranges(
        image,
        &three_tracks(),
        5_000,
        PregapPolicy::default(),
        |_| VirtualTrackId::generate(),
    )
    .unwrap();
    let queued = PlayableSource::CueTrack(ranges[1].id);
    let resolved = ResolvedSource::cue_track(&ranges[1], "album.flac".into());

    assert!(queued.is_local());
    assert_eq!(
        resolved,
        ResolvedSource::Local {
            file: image,
            path: "album.flac".into(),
            span: FrameSpan::Range(1_200..3_000)
        }
    );
}
