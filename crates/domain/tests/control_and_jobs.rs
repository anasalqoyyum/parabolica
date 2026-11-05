use std::time::{Duration, Instant};

use parabolica_domain::control::{
    CancelToken, Cancelled, GenerationCounter, SnapshotGate, SnapshotSequencer, Tagged,
};
use parabolica_domain::dispatch::coalesce;
use parabolica_domain::files::FileSignature;
use parabolica_domain::format::SampleRate;
use parabolica_domain::gain::{
    Decibels, EffectiveGainMode, GainMeasurement, GainSelection, PeakMethod, PlaybackContext,
    ReplayGainMode, StoredGains,
};
use parabolica_domain::ids::{MediaFileId, QueueEntryId, ReleaseId};
use parabolica_domain::jobs::{JobState, Progress};
use parabolica_domain::playback::{PlaybackCommand, PlaybackEvent, PositionStamp};
use parabolica_domain::tags::{
    AlbumWrite, AlbumWriteError, AlbumWriteStatus, FileWriteOutcome, TagPatch,
};

#[test]
fn results_from_superseded_generations_are_discarded() {
    let counter = GenerationCounter::default();
    let first_seek = counter.advance();
    let second_seek = counter.advance();

    assert_eq!(
        Tagged {
            generation: first_seek,
            value: 10
        }
        .accept(&counter),
        None
    );
    assert_eq!(
        Tagged {
            generation: second_seek,
            value: 20
        }
        .accept(&counter),
        Some(20)
    );
    assert!(PlaybackCommand::Seek(Duration::from_secs(3)).supersedes_pending_work());
    assert!(!PlaybackCommand::Pause.supersedes_pending_work());
}

#[test]
fn cancellation_is_shared_between_clones() {
    let requester = CancelToken::new();
    let worker = requester.clone();
    assert_eq!(worker.check(), Ok(()));
    requester.cancel();
    assert_eq!(worker.check(), Err(Cancelled));
}

#[test]
fn ui_applies_only_newer_snapshots() {
    let mut sequencer = SnapshotSequencer::default();
    let older = sequencer.wrap("loading");
    let newer = sequencer.wrap("playing");
    let mut gate = SnapshotGate::default();

    assert_eq!(gate.accept(newer.clone()), Some("playing"));
    assert_eq!(gate.accept(older), None);
    assert_eq!(gate.accept(newer), None);
}

#[test]
fn batches_keep_the_latest_value_events_and_every_discrete_event() {
    let rate = SampleRate::new(44_100).unwrap();
    let now = Instant::now();
    let stamp = |frame| {
        PlaybackEvent::Position(PositionStamp {
            frame,
            rate,
            captured_at: now,
            advancing: true,
        })
    };
    let failed = PlaybackEvent::EntryFailed {
        entry: QueueEntryId::generate(),
        reason: "missing".into(),
    };

    let batch = coalesce(vec![
        stamp(1),
        PlaybackEvent::Underruns { total: 1 },
        failed.clone(),
        stamp(2),
        PlaybackEvent::Underruns { total: 3 },
        stamp(3),
    ]);
    assert_eq!(
        batch,
        [failed, PlaybackEvent::Underruns { total: 3 }, stamp(3)]
    );
}

#[test]
fn position_extrapolates_from_the_engine_stamp() {
    let start = Instant::now();
    let rate = SampleRate::new(48_000).unwrap();
    let playing = PositionStamp {
        frame: 48_000,
        rate,
        captured_at: start,
        advancing: true,
    };
    let paused = PositionStamp {
        advancing: false,
        ..playing
    };
    let later = start + Duration::from_millis(250);

    assert_eq!(playing.position_at(later), Duration::from_millis(1_250));
    assert_eq!(paused.position_at(later), Duration::from_secs(1));
    assert_eq!(
        playing.position_at(start - Duration::from_millis(5)),
        Duration::from_secs(1)
    );
}

#[test]
fn cancelled_or_failed_jobs_never_become_complete() {
    let running = JobState::Queued
        .transition(JobState::Running(Progress {
            done: 0,
            total: Some(12),
        }))
        .unwrap();
    let cancelled = running.transition(JobState::Cancelled).unwrap();
    assert!(cancelled.is_terminal());
    assert!(cancelled.transition(JobState::Completed).is_err());
    assert!(JobState::Queued.transition(JobState::Completed).is_err());

    let completed = running.transition(JobState::Completed).unwrap();
    assert_eq!(completed.transition(JobState::Stale), Ok(JobState::Stale));
    assert!(
        JobState::Incomplete
            .transition(JobState::Completed)
            .is_err()
    );
}

fn measurement(db: f32) -> GainMeasurement {
    GainMeasurement {
        gain: Decibels::new(db).unwrap(),
        peak: None,
        peak_method: PeakMethod::Sample,
    }
}

#[test]
fn auto_gain_follows_the_playback_context() {
    assert_eq!(ReplayGainMode::default(), ReplayGainMode::Off);
    assert_eq!(
        ReplayGainMode::Auto.resolve(PlaybackContext::OrderedAlbum),
        EffectiveGainMode::Album
    );
    assert_eq!(
        ReplayGainMode::Auto.resolve(PlaybackContext::Mixed),
        EffectiveGainMode::Track
    );
    assert_eq!(
        ReplayGainMode::Album.resolve(PlaybackContext::Mixed),
        EffectiveGainMode::Album
    );
    assert_eq!(
        ReplayGainMode::Off.resolve(PlaybackContext::OrderedAlbum),
        EffectiveGainMode::Off
    );
}

#[test]
fn album_mode_never_substitutes_track_gain() {
    let track_only = StoredGains {
        track: Some(measurement(-6.0)),
        album: None,
    };
    assert_eq!(
        GainSelection::select(EffectiveGainMode::Album, &track_only),
        GainSelection::NotScanned {
            mode: EffectiveGainMode::Album
        }
    );
    assert_eq!(
        GainSelection::select(EffectiveGainMode::Track, &track_only),
        GainSelection::Measured {
            mode: EffectiveGainMode::Track,
            measurement: measurement(-6.0)
        }
    );
    assert_eq!(
        GainSelection::select(EffectiveGainMode::Off, &track_only),
        GainSelection::Bypassed
    );
    assert_eq!(Decibels::new(f32::NAN), None);
}

fn patch(target: MediaFileId) -> TagPatch {
    TagPatch {
        target,
        reviewed: FileSignature {
            size_bytes: 1,
            modified_unix_nanos: 1,
            content_digest: None,
        },
        fields: Vec::new(),
        artwork: Vec::new(),
    }
}

#[test]
fn album_writes_patch_each_physical_file_once() {
    let image = MediaFileId::generate();
    let excluded = MediaFileId::generate();
    let release = ReleaseId::generate();

    assert_eq!(
        AlbumWrite::new(
            release,
            vec![patch(image), patch(image)],
            Default::default()
        ),
        Err(AlbumWriteError::DuplicateTarget(image))
    );
    assert_eq!(
        AlbumWrite::new(release, vec![patch(excluded)], [excluded].into()),
        Err(AlbumWriteError::ExcludedTargetPatched(excluded))
    );
    assert!(AlbumWrite::new(release, vec![patch(image)], [excluded].into()).is_ok());
}

#[test]
fn album_write_status_reports_partial_outcomes() {
    let written = FileWriteOutcome::Written {
        signature: FileSignature {
            size_bytes: 2,
            modified_unix_nanos: 2,
            content_digest: None,
        },
    };
    let changed = FileWriteOutcome::ChangedSinceReview;
    assert_eq!(
        AlbumWriteStatus::from_outcomes([&written, &written]),
        AlbumWriteStatus::Complete
    );
    assert_eq!(
        AlbumWriteStatus::from_outcomes([&written, &changed]),
        AlbumWriteStatus::Partial
    );
    assert_eq!(
        AlbumWriteStatus::from_outcomes([&changed, &FileWriteOutcome::Deferred]),
        AlbumWriteStatus::Failed
    );
}
