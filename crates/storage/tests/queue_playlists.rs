mod common;

use std::path::Path;
use std::time::Duration;

use common::{file_record, queue_of, youtube};
use parabolica_domain::cue::VirtualTrackRange;
use parabolica_domain::ids::{PlaylistEntryId, QueueEntryId, VirtualTrackId};
use parabolica_domain::source::PlayableSource;
use parabolica_storage::Storage;
use parabolica_storage::playlists::m3u8::{IMPORT_BYTE_LIMIT, IMPORT_LINE_LIMIT};
use parabolica_storage::session::{ListeningRecord, SessionCheckpoint, UnavailableEntry};

#[test]
fn playlist_duplicates_reorder_crud_likes_and_all_source_identities_survive_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let database = dir.path().join("app.db");
    let storage = Storage::open(&database).unwrap();
    let root = storage.add_library_root(dir.path(), None).unwrap();
    let file = storage
        .record_media_file(file_record(root, "音楽.flac", &[("TITLE", "曲")]))
        .unwrap();
    assert_eq!(
        storage
            .register_file_path(&dir.path().join("音楽.flac"))
            .unwrap(),
        file
    );
    let cue = VirtualTrackId::generate();
    storage
        .replace_virtual_tracks(
            file,
            vec![VirtualTrackRange {
                id: cue,
                image: file,
                number: 1,
                frames: 0..100,
                index_01: 0,
            }],
        )
        .unwrap();
    let sources = vec![
        PlayableSource::LocalFile(file),
        PlayableSource::LocalFile(file),
        PlayableSource::CueTrack(cue),
        PlayableSource::Provider(youtube("remote")),
    ];
    let id = storage.create_playlist("再生").unwrap();
    let entries = storage.append_playlist(id, sources.clone()).unwrap();
    assert_ne!(entries[0], entries[1]);
    let mut order = entries.clone();
    order.reverse();
    storage.reorder_playlist(id, order.clone()).unwrap();
    let before = storage.playlist(id).unwrap();
    assert!(
        storage
            .reorder_playlist(id, vec![PlaylistEntryId::generate(); 4])
            .is_err()
    );
    assert_eq!(storage.playlist(id).unwrap(), before);
    for source in &sources {
        storage.set_local_like(source.clone(), true).unwrap();
    }
    assert_eq!(storage.local_likes().unwrap().len(), 3);
    storage.rename_playlist(id, "お気に入り").unwrap();
    drop(storage);
    let storage = Storage::open(&database).unwrap();
    let playlist = storage.playlist(id).unwrap();
    assert_eq!(playlist.name, "お気に入り");
    assert_eq!(
        playlist
            .entries
            .iter()
            .map(|entry| entry.id)
            .collect::<Vec<_>>(),
        order
    );
    assert_eq!(
        playlist
            .entries
            .iter()
            .map(|entry| entry.source.clone())
            .collect::<Vec<_>>(),
        sources.into_iter().rev().collect::<Vec<_>>()
    );
    storage.remove_playlist_entry(id, entries[0]).unwrap();
    assert_eq!(storage.playlist(id).unwrap().entries.len(), 3);
    storage
        .set_local_like(PlayableSource::LocalFile(file), false)
        .unwrap();
    assert_eq!(storage.local_likes().unwrap().len(), 2);
    storage.delete_playlist(id).unwrap();
    assert!(storage.playlists().unwrap().is_empty());
}

#[test]
fn m3u8_keeps_unicode_relative_duplicates_missing_paths_and_persistent_external_ids() {
    let dir = tempfile::tempdir().unwrap();
    let database = dir.path().join("app.db");
    let storage = Storage::open(&database).unwrap();
    let path = dir.path().join("list.m3u8");
    let bytes = "\u{feff}#EXTM3U\r\n#EXTINF:-1,曲\r\n音楽/曲.flac\r\n音楽/曲.flac\r\n../missing.wav\r\n./#music.flac\r\n".as_bytes();
    let id = storage.import_m3u8("Album", &path, bytes).unwrap();
    let playlist = storage.playlist(id).unwrap();
    assert_eq!(playlist.entries.len(), 4);
    assert_ne!(playlist.entries[0].id, playlist.entries[1].id);
    assert_eq!(playlist.entries[0].source, playlist.entries[1].source);
    let file = storage
        .register_file_path(&dir.path().join("音楽/曲.flac"))
        .unwrap();
    assert_eq!(playlist.entries[0].source, PlayableSource::LocalFile(file));
    let export = storage.export_m3u8(id, &path).unwrap();
    assert!(export.non_exportable.is_empty());
    assert_eq!(
        export.text,
        "#EXTM3U\n音楽/曲.flac\n音楽/曲.flac\n../missing.wav\n./#music.flac\n"
    );
    drop(storage);
    let storage = Storage::open(&database).unwrap();
    assert_eq!(
        storage
            .register_file_path(&dir.path().join("音楽/./曲.flac"))
            .unwrap(),
        file
    );
    assert_eq!(
        storage
            .source_details(&PlayableSource::LocalFile(file))
            .unwrap()
            .path,
        Some(dir.path().join("音楽/曲.flac"))
    );
    let root = storage.add_library_root(dir.path(), None).unwrap();
    let indexed = storage
        .record_media_file(file_record(root, "音楽/曲.flac", &[]))
        .unwrap();
    assert_ne!(indexed, file);
    assert_eq!(
        storage
            .register_file_path(&dir.path().join("音楽/曲.flac"))
            .unwrap(),
        indexed
    );
    assert_eq!(storage.playlist(id).unwrap(), playlist);
    assert_eq!(
        storage
            .source_details(&PlayableSource::LocalFile(file))
            .unwrap()
            .path,
        Some(dir.path().join("音楽/曲.flac"))
    );
}

#[test]
fn m3u8_reports_every_non_exportable_identity_and_rejects_unsafe_or_unbounded_inputs() {
    let dir = tempfile::tempdir().unwrap();
    let storage = Storage::open(dir.path().join("app.db")).unwrap();
    let id = storage.create_playlist("Mixed").unwrap();
    let entries = storage
        .append_playlist(
            id,
            vec![
                PlayableSource::Provider(youtube("remote")),
                PlayableSource::CueTrack(VirtualTrackId::generate()),
            ],
        )
        .unwrap();
    let export = storage
        .export_m3u8(id, &dir.path().join("out.m3u8"))
        .unwrap();
    assert_eq!(
        export
            .non_exportable
            .iter()
            .map(|entry| entry.entry)
            .collect::<Vec<_>>(),
        entries
    );
    for bytes in [
        vec![0xff],
        vec![b'a'; IMPORT_BYTE_LIMIT + 1],
        vec![b'a'; IMPORT_LINE_LIMIT + 1],
        b"good.flac\ninvalid\0path.flac".to_vec(),
        b"bad\rname.flac".to_vec(),
        b"https://example.com/music".to_vec(),
        "x.flac\n".repeat(10_001).into_bytes(),
    ] {
        assert!(
            storage
                .import_m3u8("Bad", &dir.path().join("bad.m3u8"), &bytes)
                .is_err()
        );
    }
    assert_eq!(storage.playlists().unwrap().len(), 1);
    assert!(
        storage
            .register_file_path(Path::new("injected\nnext.flac"))
            .is_err()
    );
}

#[test]
fn a_session_checkpoint_rolls_back_queue_history_navigation_and_retry_state_together() {
    let dir = tempfile::tempdir().unwrap();
    let database = dir.path().join("app.db");
    let storage = Storage::open(&database).unwrap();
    let queue = queue_of(
        vec![
            PlayableSource::Provider(youtube("a")),
            PlayableSource::Provider(youtube("b")),
        ],
        Duration::from_micros(321_456),
    );
    let current = queue.current.unwrap();
    let mut session = SessionCheckpoint {
        queue,
        navigation: vec![current],
        unavailable: vec![UnavailableEntry {
            entry: current,
            reason: "Offline".into(),
            retry_attempts: 2,
        }],
        history: vec![ListeningRecord {
            entry: current,
            source: PlayableSource::Provider(youtube("played")),
            started_at_ms: 123,
        }],
    };
    storage.save_session(session.clone()).unwrap();
    let mut bad = session.clone();
    bad.queue.position = Duration::from_secs(10);
    bad.history.push(ListeningRecord {
        entry: QueueEntryId::generate(),
        source: PlayableSource::Provider(youtube("")),
        started_at_ms: 124,
    });
    assert!(storage.save_session(bad).is_err());
    assert_eq!(storage.load_session().unwrap(), Some(session.clone()));
    drop(storage);
    let storage = Storage::open(&database).unwrap();
    assert_eq!(storage.load_session().unwrap(), Some(session.clone()));
    assert!(
        !storage
            .save_playback_position(QueueEntryId::generate(), Duration::from_secs(99))
            .unwrap()
    );
    session.queue.entries.clear();
    session.queue.current = None;
    session.queue.shuffle = None;
    session.queue.position = Duration::ZERO;
    session.navigation.clear();
    session.unavailable.clear();
    storage.save_session(session.clone()).unwrap();
    assert_eq!(storage.load_session().unwrap(), Some(session));
}

#[test]
fn append_failure_rolls_back_earlier_duplicate_insertions() {
    let dir = tempfile::tempdir().unwrap();
    let storage = Storage::open(dir.path().join("app.db")).unwrap();
    let id = storage.create_playlist("Stable").unwrap();
    assert!(
        storage
            .append_playlist(
                id,
                vec![
                    PlayableSource::Provider(youtube("valid")),
                    PlayableSource::Provider(youtube(""))
                ]
            )
            .is_err()
    );
    assert!(storage.playlist(id).unwrap().entries.is_empty());
}
