//! Create, reopen and use the database through the public storage contract.

mod common;

use std::path::Path;
use std::time::Duration;

use common::{AppearanceMode, database_files, file_record, queue_of, signature, youtube};
use parabolica_domain::cue::VirtualTrackRange;
use parabolica_domain::files::{FileLocation, OsFileId, RootAvailability, VolumeId};
use parabolica_domain::ids::{QueueEntryId, VirtualTrackId};
use parabolica_domain::jobs::{JobKind, JobState, Progress};
use parabolica_domain::source::PlayableSource;
use parabolica_storage::jobs::{ItemOutcome, ItemReport, NewJobItem};
use parabolica_storage::library::{DisplayTag, Presence};
use parabolica_storage::preferences::PreferenceBatch;
use parabolica_storage::sources::LinkState;
use parabolica_storage::{Storage, StorageError};

#[test]
fn library_queue_and_preferences_survive_reopening() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("data").join("parabolica.db");

    let storage = Storage::open(&path).unwrap();
    let root = storage
        .add_library_root(Path::new("D:/Music"), Some(VolumeId("vol-1".into())))
        .unwrap();
    let image = storage
        .record_media_file(file_record(
            root,
            "Artist/Album/image.flac",
            &[
                ("ALBUM", "Album"),
                ("ARTIST", "First"),
                ("ARTIST", "Second"),
            ],
        ))
        .unwrap();
    let cue_track = VirtualTrackRange {
        id: VirtualTrackId::generate(),
        image,
        number: 1,
        frames: 0..44_100,
        index_01: 0,
    };
    storage
        .replace_virtual_tracks(image, vec![cue_track.clone()])
        .unwrap();
    let queue = queue_of(
        vec![
            PlayableSource::LocalFile(image),
            PlayableSource::CueTrack(cue_track.id),
            PlayableSource::Provider(youtube("abc")),
            PlayableSource::LocalFile(image),
        ],
        Duration::from_millis(83_250),
    );
    storage.save_queue(queue.clone()).unwrap();
    storage
        .save_preferences(PreferenceBatch::new().set(&AppearanceMode::Dark))
        .unwrap();
    drop(storage);

    let storage = Storage::open(&path).unwrap();
    assert_eq!(storage.library_roots().unwrap()[0].id, root);
    assert_eq!(storage.virtual_tracks(image).unwrap(), vec![cue_track]);
    assert_eq!(
        storage.media_file_tags(image).unwrap(),
        vec![
            DisplayTag::new("ALBUM", "Album"),
            DisplayTag::new("ARTIST", "First"),
            DisplayTag::new("ARTIST", "Second"),
        ]
    );
    assert_eq!(storage.load_queue().unwrap(), Some(queue));
    assert_eq!(
        storage.preference::<AppearanceMode>().unwrap(),
        Some(AppearanceMode::Dark)
    );
    drop(storage);

    // Reopening without pending migrations does not take a backup.
    let storage = Storage::open(&path).unwrap();
    drop(storage);
    assert!(
        !database_files(path.parent().unwrap())
            .iter()
            .any(|name| name.contains(".backup-"))
    );
}

#[test]
fn a_file_keeps_its_identity_across_retagging_moves_and_an_offline_root() {
    let dir = tempfile::tempdir().unwrap();
    let storage = Storage::open(dir.path().join("parabolica.db")).unwrap();
    let root = storage
        .add_library_root(Path::new("E:/Music"), None)
        .unwrap();
    assert_eq!(
        storage
            .add_library_root(Path::new("E:/Music"), None)
            .unwrap(),
        root
    );

    let os_id = OsFileId {
        volume: VolumeId("vol-e".into()),
        file_id: 0x0123_4567_89ab_cdef_0011_2233_4455_6677,
    };
    let mut record = file_record(root, "A/01.flac", &[("TITLE", "Old")]);
    record.os_file_id = Some(os_id.clone());
    let file = storage.record_media_file(record.clone()).unwrap();

    record.tags = vec![DisplayTag::new("title", "New")];
    record.signature = signature(2_000);
    assert_eq!(storage.record_media_file(record.clone()).unwrap(), file);
    assert_eq!(
        storage.media_file_tags(file).unwrap(),
        vec![DisplayTag::new("TITLE", "New")]
    );
    assert_eq!(
        storage.media_file(file).unwrap().unwrap().signature,
        signature(2_000)
    );

    // Seen at a new path with the same OS file ID after the old path went missing: a move.
    storage
        .set_media_file_presence(file, Presence::Missing)
        .unwrap();
    let mut moved = record.clone();
    moved.location.relative_path = ["B", "01.flac"].iter().collect();
    assert_eq!(storage.record_media_file(moved).unwrap(), file);
    let stored = storage.media_file(file).unwrap().unwrap();
    assert_eq!(stored.presence, Presence::Present);
    assert_eq!(stored.os_file_id, Some(os_id));
    assert_eq!(
        stored.location.relative_path,
        ["B", "01.flac"].iter().collect::<std::path::PathBuf>()
    );

    // An explicit move without an OS file ID keeps identity too.
    let destination = FileLocation {
        root,
        relative_path: ["C", "01.flac"].iter().collect(),
    };
    storage.move_media_file(file, destination.clone()).unwrap();
    assert_eq!(
        storage.media_file(file).unwrap().unwrap().location,
        destination
    );

    // Unplugging the drive marks the root, not its files.
    storage
        .set_root_availability(root, RootAvailability::Offline)
        .unwrap();
    assert_eq!(
        storage.library_roots().unwrap()[0].availability,
        RootAvailability::Offline
    );
    assert_eq!(
        storage.media_file(file).unwrap().unwrap().presence,
        Presence::Present
    );
}

#[test]
fn search_matches_substrings_in_any_script_and_ignores_diacritics() {
    let dir = tempfile::tempdir().unwrap();
    let storage = Storage::open(dir.path().join("parabolica.db")).unwrap();
    let root = storage.add_library_root(Path::new("/music"), None).unwrap();
    let japanese = storage
        .record_media_file(file_record(
            root,
            "utada/first-love.flac",
            &[
                ("TITLE", "First Love"),
                ("ARTIST", "宇多田ヒカル"),
                ("ALBUM", "First Love"),
            ],
        ))
        .unwrap();
    let indonesian = storage
        .record_media_file(file_record(
            root,
            "chrisye/lilin.flac",
            &[
                ("TITLE", "Lilin-Lilin Kecil"),
                ("ARTIST", "Chrisye"),
                ("GENRE", "Pop"),
            ],
        ))
        .unwrap();
    let accented = storage
        .record_media_file(file_record(
            root,
            "sigur/hoppipolla.flac",
            &[
                ("TITLE", "Hoppípolla"),
                ("ARTIST", "Sigur Rós"),
                ("ALBUM", "Takk..."),
            ],
        ))
        .unwrap();
    let untagged = storage
        .record_media_file(file_record(root, "inbox/Field Recording 03.flac", &[]))
        .unwrap();

    let composed = storage
        .record_media_file(file_record(
            root,
            "misc/dragon.flac",
            &[
                ("TITLE", "가나다라 ドラゴン"),
                ("ARTIST", "Cafe\u{301} Tacvba"),
            ],
        ))
        .unwrap();
    let search = |query: &str| storage.search_media_files(query, 50).unwrap();
    // Hangul and voiced kana keep their composed form; an NFD tag matches an NFC query.
    assert_eq!(search("가나다"), vec![composed]);
    assert_eq!(search("ドラゴン"), vec![composed]);
    assert_eq!(search("ドラ"), vec![composed]);
    assert_eq!(search("café"), vec![composed]);
    assert_eq!(search("tacvba cafe"), vec![composed]);
    assert_eq!(search("宇多田"), vec![japanese]);
    assert_eq!(search("first love"), vec![japanese]);
    assert_eq!(search("hoppipolla ros"), vec![accented]);
    assert_eq!(search("lilin pop"), vec![indonesian]);
    assert_eq!(search("recording"), vec![untagged]);
    // Short words are matched without the trigram index, folded the same way.
    assert_eq!(search("Ró"), vec![accented]);
    assert_eq!(search("hoppipolla ro"), vec![accented]);
    assert_eq!(search("HOPPIPOLLA RÓ"), vec![accented]);
    assert_eq!(search("Ro\u{301}"), vec![accented]);
    assert_eq!(search("Hoppi\u{301}polla"), vec![accented]);
    assert_eq!(search("宇多"), vec![japanese]);
    assert_eq!(search("lilin 50%"), Vec::new());
    // Query syntax in user input is matched literally.
    assert_eq!(search("\"chrisye\" OR NOT"), Vec::new());
    assert_eq!(search("   "), Vec::new());

    // Retagging replaces the indexed text.
    storage
        .record_media_file(file_record(
            root,
            "chrisye/lilin.flac",
            &[("TITLE", "Badai Pasti Berlalu")],
        ))
        .unwrap();
    assert_eq!(search("lilin pop"), Vec::new());
    assert_eq!(search("badai"), vec![indonesian]);

    // Moving a file re-indexes its file name.
    storage
        .move_media_file(
            untagged,
            FileLocation {
                root,
                relative_path: ["inbox", "Night Take.flac"].iter().collect(),
            },
        )
        .unwrap();
    assert_eq!(search("recording"), Vec::new());
    assert_eq!(search("night take"), vec![untagged]);

    // Removing the root removes its files from search.
    storage.remove_library_root(root).unwrap();
    assert_eq!(search("badai"), Vec::new());
}

#[test]
fn source_links_keep_one_recording_per_source_and_remember_rejections() {
    let dir = tempfile::tempdir().unwrap();
    let storage = Storage::open(dir.path().join("parabolica.db")).unwrap();
    let root = storage.add_library_root(Path::new("/music"), None).unwrap();
    let file = storage
        .record_media_file(file_record(root, "a.flac", &[("TITLE", "Song")]))
        .unwrap();
    let recording = storage.create_track().unwrap();
    let other = storage.create_track().unwrap();
    let local = PlayableSource::LocalFile(file);
    let remote = PlayableSource::Provider(youtube("video-1"));

    storage
        .link_source(recording, local.clone(), LinkState::Confirmed, None)
        .unwrap();
    storage
        .link_source(
            recording,
            remote.clone(),
            LinkState::Suggested,
            Some("same ISRC".into()),
        )
        .unwrap();
    assert!(matches!(
        storage.link_source(other, remote.clone(), LinkState::Confirmed, None),
        Err(StorageError::Conflict(_))
    ));

    // The user rejects the suggestion; it stays recorded for this recording only.
    storage
        .link_source(recording, remote.clone(), LinkState::Rejected, None)
        .unwrap();
    assert_eq!(storage.source_link(&remote).unwrap(), None);
    storage
        .link_source(other, remote.clone(), LinkState::Confirmed, None)
        .unwrap();
    assert_eq!(storage.source_link(&remote).unwrap().unwrap().track, other);
    let links = storage.track_sources(recording).unwrap();
    assert_eq!(links.len(), 2);
    assert_eq!(links[1].state, LinkState::Rejected);
    assert_eq!(links[1].source, remote);

    // Distinct sources of each kind link independently.
    let [second_file, third_file] = ["b.flac", "c.flac"].map(|path| {
        storage
            .record_media_file(file_record(root, path, &[]))
            .unwrap()
    });
    let image = storage
        .record_media_file(file_record(root, "image.flac", &[]))
        .unwrap();
    let cue_tracks: Vec<VirtualTrackRange> = (1..=2)
        .map(|number| VirtualTrackRange {
            id: VirtualTrackId::generate(),
            image,
            number,
            frames: u64::from(number) * 100..u64::from(number) * 100 + 100,
            index_01: u64::from(number) * 100,
        })
        .collect();
    storage
        .replace_virtual_tracks(image, cue_tracks.clone())
        .unwrap();
    for (first, second) in [
        (
            PlayableSource::Provider(youtube("video-2")),
            PlayableSource::Provider(youtube("video-3")),
        ),
        (
            PlayableSource::CueTrack(cue_tracks[0].id),
            PlayableSource::CueTrack(cue_tracks[1].id),
        ),
        (
            PlayableSource::LocalFile(second_file),
            PlayableSource::LocalFile(third_file),
        ),
    ] {
        let first_track = storage.create_track().unwrap();
        let second_track = storage.create_track().unwrap();
        storage
            .link_source(first_track, first.clone(), LinkState::Suggested, None)
            .unwrap();
        storage
            .link_source(second_track, second.clone(), LinkState::Confirmed, None)
            .unwrap();
        assert_eq!(
            storage.source_link(&first).unwrap().unwrap().track,
            first_track
        );
        assert_eq!(
            storage.source_link(&second).unwrap().unwrap().track,
            second_track
        );
    }

    // Removing the file removes its links but not the recording's other links.
    storage.remove_library_root(root).unwrap();
    assert_eq!(storage.source_link(&local).unwrap(), None);
    assert_eq!(storage.track_sources(recording).unwrap().len(), 1);
}

#[test]
fn queue_checkpoints_reject_broken_invariants_and_ignore_stale_positions() {
    let dir = tempfile::tempdir().unwrap();
    let storage = Storage::open(dir.path().join("parabolica.db")).unwrap();
    assert_eq!(storage.load_queue().unwrap(), None);

    let sources = vec![
        PlayableSource::Provider(youtube("a")),
        PlayableSource::Provider(youtube("b")),
        PlayableSource::Provider(youtube("a")),
    ];
    let queue = queue_of(sources, Duration::from_secs(5));
    storage.save_queue(queue.clone()).unwrap();

    let mut duplicate = queue.clone();
    duplicate.entries[2].id = duplicate.entries[0].id;
    let mut foreign_current = queue.clone();
    foreign_current.current = Some(QueueEntryId::generate());
    let mut partial_shuffle = queue.clone();
    partial_shuffle.shuffle.as_mut().unwrap().order.pop();
    for broken in [duplicate, foreign_current, partial_shuffle] {
        assert!(matches!(
            storage.save_queue(broken),
            Err(StorageError::Invalid { .. })
        ));
    }
    assert_eq!(storage.load_queue().unwrap(), Some(queue.clone()));

    let current = queue.current.unwrap();
    assert!(
        storage
            .save_playback_position(current, Duration::from_secs(9))
            .unwrap()
    );
    assert!(
        !storage
            .save_playback_position(queue.entries[0].id, Duration::from_secs(1))
            .unwrap()
    );
    let restored = storage.load_queue().unwrap().unwrap();
    assert_eq!(restored.position, Duration::from_secs(9));
    assert_eq!(restored.entries, queue.entries);
}

#[test]
fn jobs_resume_after_reopening_and_never_complete_with_unfinished_items() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("parabolica.db");
    let storage = Storage::open(&path).unwrap();
    let items = ["one", "two", "three"].map(|key| NewJobItem {
        key: key.to_owned(),
        file: None,
    });
    let job = storage
        .create_job(JobKind::ReplayGainScan, items.to_vec())
        .unwrap();
    storage
        .transition_job(
            job,
            JobState::Running(Progress {
                done: 0,
                total: Some(3),
            }),
        )
        .unwrap();
    storage
        .record_job_items(
            job,
            vec![ItemReport {
                key: "one".into(),
                outcome: ItemOutcome::Done,
                detail: None,
                signature: Some(signature(10)),
            }],
        )
        .unwrap();
    drop(storage);

    // After a restart the job is still running with one item done.
    let storage = Storage::open(&path).unwrap();
    let unfinished = storage.unfinished_jobs().unwrap();
    assert_eq!(unfinished.len(), 1);
    assert_eq!(
        unfinished[0].state,
        JobState::Running(Progress {
            done: 1,
            total: Some(3)
        })
    );
    let pending: Vec<_> = storage
        .job_items(job)
        .unwrap()
        .into_iter()
        .filter(|item| item.outcome == ItemOutcome::Pending)
        .map(|item| item.key)
        .collect();
    assert_eq!(pending, ["two", "three"]);
    assert_eq!(
        storage.job_items(job).unwrap()[0].signature,
        Some(signature(10))
    );

    storage
        .record_job_items(
            job,
            vec![
                ItemReport {
                    key: "two".into(),
                    outcome: ItemOutcome::Done,
                    detail: None,
                    signature: None,
                },
                ItemReport {
                    key: "three".into(),
                    outcome: ItemOutcome::Failed,
                    detail: Some("decode error at 01:02".into()),
                    signature: None,
                },
            ],
        )
        .unwrap();
    assert!(matches!(
        storage.transition_job(job, JobState::Completed),
        Err(StorageError::Invalid { .. })
    ));
    storage.transition_job(job, JobState::Incomplete).unwrap();
    assert!(matches!(
        storage.transition_job(job, JobState::Completed),
        Err(StorageError::JobTransition { .. })
    ));
    assert!(storage.unfinished_jobs().unwrap().is_empty());
    let record = storage.job(job).unwrap().unwrap();
    assert_eq!(record.state, JobState::Incomplete);
    assert_eq!(
        record.progress,
        Progress {
            done: 3,
            total: Some(3)
        }
    );

    // A report that arrives after cancellation changes nothing.
    let cancelled = storage
        .create_job(JobKind::TagWrite, items.to_vec())
        .unwrap();
    storage
        .transition_job(
            cancelled,
            JobState::Running(Progress {
                done: 0,
                total: Some(3),
            }),
        )
        .unwrap();
    storage
        .transition_job(cancelled, JobState::Cancelled)
        .unwrap();
    let late = ItemReport {
        key: "one".into(),
        outcome: ItemOutcome::Done,
        detail: None,
        signature: None,
    };
    assert!(matches!(
        storage.record_job_items(cancelled, vec![late]),
        Err(StorageError::Conflict(_))
    ));
    assert!(
        storage
            .job_items(cancelled)
            .unwrap()
            .iter()
            .all(|item| item.outcome == ItemOutcome::Pending)
    );

    // A completed job can later go stale.
    let quick = storage.create_job(JobKind::Backup, Vec::new()).unwrap();
    storage
        .transition_job(
            quick,
            JobState::Running(Progress {
                done: 0,
                total: None,
            }),
        )
        .unwrap();
    storage.transition_job(quick, JobState::Completed).unwrap();
    storage.transition_job(quick, JobState::Stale).unwrap();
    let failed = storage.create_job(JobKind::TagWrite, Vec::new()).unwrap();
    storage
        .transition_job(
            failed,
            JobState::Running(Progress {
                done: 0,
                total: None,
            }),
        )
        .unwrap();
    storage
        .transition_job(
            failed,
            JobState::Failed {
                reason: "read-only file".into(),
            },
        )
        .unwrap();
    assert_eq!(
        storage.job(failed).unwrap().unwrap().state,
        JobState::Failed {
            reason: "read-only file".into()
        }
    );
    // A rejected transition names the states without the stored reason.
    let error = storage
        .transition_job(failed, JobState::Completed)
        .unwrap_err();
    assert_eq!(
        error.to_string(),
        "job cannot move from failed to completed"
    );
    assert!(!format!("{error:?}").contains("read-only"));
}

#[test]
fn replacing_cue_tracks_can_renumber_retained_tracks_and_keeps_their_links() {
    let dir = tempfile::tempdir().unwrap();
    let storage = Storage::open(dir.path().join("parabolica.db")).unwrap();
    let root = storage.add_library_root(Path::new("/music"), None).unwrap();
    let image = storage
        .record_media_file(file_record(root, "image.flac", &[]))
        .unwrap();
    let range = |id, number: u8, start: u64| VirtualTrackRange {
        id,
        image,
        number,
        frames: start..start + 1_000,
        index_01: start,
    };
    let (first, second) = (VirtualTrackId::generate(), VirtualTrackId::generate());
    storage
        .replace_virtual_tracks(image, vec![range(first, 1, 0), range(second, 2, 1_000)])
        .unwrap();
    let recording = storage.create_track().unwrap();
    storage
        .link_source(
            recording,
            PlayableSource::CueTrack(second),
            LinkState::Confirmed,
            None,
        )
        .unwrap();

    // A revised sheet inserts a new first track and shifts the retained ones.
    let inserted = VirtualTrackId::generate();
    let revised = vec![
        range(first, 2, 1_000),
        range(second, 3, 2_000),
        range(inserted, 1, 0),
    ];
    storage.replace_virtual_tracks(image, revised).unwrap();
    let numbers: Vec<_> = storage
        .virtual_tracks(image)
        .unwrap()
        .iter()
        .map(|track| (track.number, track.id))
        .collect();
    assert_eq!(numbers, [(1, inserted), (2, first), (3, second)]);
    assert_eq!(
        storage
            .source_link(&PlayableSource::CueTrack(second))
            .unwrap()
            .unwrap()
            .track,
        recording
    );

    assert!(matches!(
        storage.replace_virtual_tracks(image, vec![range(first, 1, 0), range(second, 1, 1_000)]),
        Err(StorageError::Invalid { .. })
    ));
}
