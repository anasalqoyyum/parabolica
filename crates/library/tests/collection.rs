use std::fs;
use std::io::Cursor;
use std::path::Path;
use std::time::Duration;

use parabolica_domain::control::CancelToken;
use parabolica_domain::files::RootAvailability;
use parabolica_domain::ids::MediaFileId;
use parabolica_library::service::{Command, LibraryService};
use parabolica_library::{artwork, index, metadata};
use parabolica_storage::{
    Storage,
    collection::{BrowseKind, BrowseQuery, ItemId, Sort},
    library::Presence,
};

fn flac(path: &Path, tags: &[(&str, &str)]) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let fixture =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../audio/tests/fixtures/flac/s16_44100.flac");
    let original = fs::read(fixture).unwrap();
    let mut data = original[..42].to_vec();
    data[4] &= 127;
    let mut comments = Vec::new();
    comments.extend(0u32.to_le_bytes());
    comments.extend((tags.len() as u32).to_le_bytes());
    for (key, value) in tags {
        let value = format!("{key}={value}");
        comments.extend((value.len() as u32).to_le_bytes());
        comments.extend(value.bytes());
    }
    data.push(128 | 4);
    data.extend((comments.len() as u32).to_be_bytes()[1..].iter());
    data.extend(comments);
    // Keep the synthetic fixture's audio frames after its metadata.
    let mut end = 4;
    loop {
        let last = original[end] & 128 != 0;
        let size = u32::from_be_bytes([0, original[end + 1], original[end + 2], original[end + 3]])
            as usize;
        end += 4 + size;
        if last {
            break;
        }
    }
    data.extend(&original[end..]);
    fs::write(path, data).unwrap();
}
fn scan(storage: &Storage) {
    for root in storage.library_roots().unwrap() {
        index::scan(storage, &root, &CancelToken::new(), |_| {}).unwrap();
    }
}
fn songs(storage: &Storage) -> Vec<MediaFileId> {
    storage
        .browse_collection(
            BrowseKind::Songs,
            BrowseQuery {
                limit: 256,
                ..Default::default()
            },
        )
        .unwrap()
        .into_iter()
        .map(|i| match i.id {
            ItemId::Song(id) => id,
            _ => panic!("song"),
        })
        .collect()
}
fn album_ids(storage: &Storage) -> Vec<parabolica_domain::ids::ReleaseId> {
    storage
        .browse_collection(
            BrowseKind::Albums,
            BrowseQuery {
                limit: 256,
                ..Default::default()
            },
        )
        .unwrap()
        .into_iter()
        .map(|i| match i.id {
            ItemId::Album(id) => id,
            _ => panic!("album"),
        })
        .collect()
}

#[test]
fn incremental_index_search_multidisc_compilations_and_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("音楽");
    let db = dir.path().join("library.db");
    flac(
        &root.join("集/CD1/01.flac"),
        &[
            ("TITLE", "Été 東京"),
            ("ALBUM", "Live"),
            ("ALBUMARTIST", "Various artists"),
            ("ARTIST", "First"),
            ("COMPILATION", "1"),
            ("DISCNUMBER", "1"),
            ("TRACKNUMBER", "2"),
        ],
    );
    flac(
        &root.join("集/CD2/01.flac"),
        &[
            ("TITLE", "Kedua"),
            ("ALBUM", "Live"),
            ("ALBUMARTIST", "Various artists"),
            ("ARTIST", "Second"),
            ("DISCNUMBER", "2"),
            ("TRACKNUMBER", "1"),
        ],
    );
    flac(
        &root.join("Edition/01.flac"),
        &[
            ("TITLE", "Bonus"),
            ("ALBUM", "Live"),
            ("ARTIST", "First"),
            ("CATALOGNUMBER", "DELUXE"),
        ],
    );
    let storage = Storage::open(&db).unwrap();
    let id = index::add_folder(&storage, &root).unwrap();
    let mut increments = Vec::new();
    index::scan(
        &storage,
        &storage.library_roots().unwrap()[0],
        &CancelToken::new(),
        |p| {
            if p.indexed > 0 {
                increments.push(songs(&storage).len());
            }
        },
    )
    .unwrap();
    assert!(increments.contains(&1));
    assert_eq!(songs(&storage).len(), 3);
    assert_eq!(album_ids(&storage).len(), 2);
    let results = storage
        .browse_collection(
            BrowseKind::Songs,
            BrowseQuery {
                text: "ete 東京".into(),
                limit: 10,
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].title, "Été 東京");
    assert_eq!(
        storage
            .browse_collection(
                BrowseKind::Artists,
                BrowseQuery {
                    limit: 10,
                    ..Default::default()
                }
            )
            .unwrap()
            .len(),
        2
    );
    let files = songs(&storage);
    let albums = album_ids(&storage);
    let progress = index::scan(
        &storage,
        &storage.library_roots().unwrap()[0],
        &CancelToken::new(),
        |_| {},
    )
    .unwrap();
    assert_eq!(progress.unchanged, 3);
    assert_eq!(progress.indexed, 0);
    drop(storage);
    let storage = Storage::open(&db).unwrap();
    assert_eq!(songs(&storage), files);
    assert_eq!(album_ids(&storage), albums);
    assert_eq!(storage.library_roots().unwrap()[0].id, id);
}

#[test]
fn offline_reconnect_cancel_and_rename_preserve_identity() {
    let dir = tempfile::tempdir().unwrap();
    let music = dir.path().join("Music");
    flac(
        &music.join("Album/one.flac"),
        &[("ALBUM", "Album"), ("ARTIST", "Artist")],
    );
    let storage = Storage::open(dir.path().join("db")).unwrap();
    let root_id = index::add_folder(&storage, &music).unwrap();
    scan(&storage);
    let file = songs(&storage)[0];
    let album = album_ids(&storage)[0];
    let root = storage.library_roots().unwrap().remove(0);
    fs::rename(music.join("Album"), music.join("Moved")).unwrap();
    scan(&storage);
    assert_eq!(songs(&storage), vec![file]);
    assert_eq!(album_ids(&storage), vec![album]);
    fs::remove_file(music.join("Moved/one.flac")).unwrap();
    let token = CancelToken::new();
    token.cancel();
    assert!(index::scan(&storage, &root, &token, |_| {}).is_err());
    assert_eq!(
        storage.media_file(file).unwrap().unwrap().presence,
        Presence::Present
    );
    scan(&storage);
    assert_eq!(
        storage.media_file(file).unwrap().unwrap().presence,
        Presence::Missing
    );
    flac(
        &music.join("Moved/one.flac"),
        &[("ALBUM", "Album"), ("ARTIST", "Artist")],
    );
    scan(&storage);
    assert_eq!(songs(&storage), vec![file]);
    fs::rename(&music, dir.path().join("Unplugged")).unwrap();
    assert!(index::scan(&storage, &root, &CancelToken::new(), |_| {}).is_err());
    assert_eq!(
        storage.library_roots().unwrap()[0].availability,
        RootAvailability::Offline
    );
    assert_eq!(
        storage.media_file(file).unwrap().unwrap().presence,
        Presence::Present
    );
    index::relocate_folder(&storage, root_id, &dir.path().join("Unplugged")).unwrap();
    scan(&storage);
    assert_eq!(songs(&storage), vec![file]);
    assert_eq!(album_ids(&storage), vec![album]);
    assert!(storage.resolve_local_file(file).unwrap().is_file());
}

#[test]
fn manual_grouping_and_reset_survive_rescans_without_touching_source() {
    let dir = tempfile::tempdir().unwrap();
    let music = dir.path().join("Music");
    for name in ["One", "Two"] {
        flac(
            &music.join(name).join("a.flac"),
            &[("ALBUM", name), ("ARTIST", "Artist")],
        );
    }
    let storage = Storage::open(dir.path().join("db")).unwrap();
    index::add_folder(&storage, &music).unwrap();
    scan(&storage);
    let files = songs(&storage);
    let albums = album_ids(&storage);
    storage.group_local_files(files.clone(), albums[1]).unwrap();
    scan(&storage);
    assert_eq!(album_ids(&storage).len(), 1);
    storage.reset_local_grouping(files.clone()).unwrap();
    assert_eq!(album_ids(&storage).len(), 2);
    assert_eq!(songs(&storage), files);
}

#[test]
fn corrupt_metadata_is_visible_and_huge_art_does_not_block_tags() {
    let dir = tempfile::tempdir().unwrap();
    let music = dir.path().join("Music");
    let path = music.join("a.flac");
    flac(&path, &[("TITLE", "Readable"), ("ALBUM", "Art")]);
    let mut data = fs::read(&path).unwrap();
    data[42] &= 127;
    let comment_size = u32::from_be_bytes([0, data[43], data[44], data[45]]) as usize;
    let end = 46 + comment_size;
    let size = 9 * 1024 * 1024;
    let mut picture = vec![128 | 6];
    picture.extend((size as u32).to_be_bytes()[1..].iter());
    picture.resize(size + 4, 0);
    data.splice(end..end, picture);
    fs::write(&path, data).unwrap();
    assert_eq!(metadata::read(&path).unwrap().tags[0].value, "Readable");
    fs::write(music.join("broken.flac"), b"fLaC\x80\xff\xff\xff").unwrap();
    let storage = Storage::open(dir.path().join("db")).unwrap();
    index::add_folder(&storage, &music).unwrap();
    scan(&storage);
    assert_eq!(songs(&storage).len(), 2);
    assert_eq!(
        storage
            .browse_collection(
                BrowseKind::Songs,
                BrowseQuery {
                    available_only: true,
                    limit: 10,
                    ..Default::default()
                }
            )
            .unwrap()
            .len(),
        1
    );
    let id = songs(&storage)[0];
    let record = storage.media_file(id).unwrap().unwrap();
    let mut cache = artwork::ThumbnailCache::default();
    let _ = cache
        .load(id, record.signature, &path, &CancelToken::new())
        .unwrap();
}

#[test]
fn thumbnail_limits_and_cancel_are_enforced() {
    let image = image::DynamicImage::new_rgb8(1000, 500);
    let mut bytes = Cursor::new(Vec::new());
    image.write_to(&mut bytes, image::ImageFormat::Png).unwrap();
    let thumb = artwork::thumbnail(bytes.get_ref(), &CancelToken::new()).unwrap();
    let decoded = image::load_from_memory(&thumb).unwrap();
    assert_eq!((decoded.width(), decoded.height()), (256, 128));
    let token = CancelToken::new();
    token.cancel();
    assert!(artwork::thumbnail(bytes.get_ref(), &token).is_err());
    assert!(artwork::thumbnail(&vec![0; 9 * 1024 * 1024], &CancelToken::new()).is_err());
}

#[test]
fn paged_fts_and_filters_keep_all_results() {
    let dir = tempfile::tempdir().unwrap();
    let music = dir.path().join("Music");
    for n in 0..10 {
        flac(
            &music.join(format!("{n}.flac")),
            &[("TITLE", &format!("Common {n}")), ("ALBUM", "One")],
        );
    }
    let storage = Storage::open(dir.path().join("db")).unwrap();
    index::add_folder(&storage, &music).unwrap();
    scan(&storage);
    let mut query = BrowseQuery {
        text: "common".into(),
        limit: 3,
        sort: Sort::Title,
        ..Default::default()
    };
    let mut ids = Vec::new();
    loop {
        let page = storage
            .browse_collection(BrowseKind::Songs, query.clone())
            .unwrap();
        if page.is_empty() {
            break;
        }
        query.offset += page.len();
        ids.extend(page.into_iter().map(|i| i.id));
    }
    assert_eq!(ids.len(), 10);
}

#[test]
fn watchers_index_added_changed_removed_files_and_cancel_can_resume() {
    let dir = tempfile::tempdir().unwrap();
    let music = dir.path().join("Music");
    fs::create_dir_all(&music).unwrap();
    let storage = Storage::open(dir.path().join("db")).unwrap();
    index::add_folder(&storage, &music).unwrap();
    let service = LibraryService::start(storage.clone());
    wait(|| !service.status().scanning && service.status().revision > 0);
    flac(&music.join("one.flac"), &[("TITLE", "Before")]);
    wait(|| songs(&storage).len() == 1);
    let id = songs(&storage)[0];
    flac(&music.join("one.flac"), &[("TITLE", "After")]);
    wait(|| {
        storage
            .browse_collection(
                BrowseKind::Songs,
                BrowseQuery {
                    text: "After".into(),
                    limit: 10,
                    ..Default::default()
                },
            )
            .unwrap()
            .len()
            == 1
    });
    fs::remove_file(music.join("one.flac")).unwrap();
    wait(|| storage.media_file(id).unwrap().unwrap().presence == Presence::Missing);
    service.cancel();
    service.send(Command::Rescan).unwrap();
    wait(|| !service.status().scanning);
}
fn wait(mut predicate: impl FnMut() -> bool) {
    let deadline = std::time::Instant::now() + Duration::from_secs(12);
    while !predicate() {
        assert!(
            std::time::Instant::now() < deadline,
            "worker did not settle"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn unicode_long_paths_index_without_following_symlink_cycles() {
    let dir = tempfile::tempdir().unwrap();
    let music = dir.path().join("Music");
    fs::create_dir(&music).unwrap();
    let music = fs::canonicalize(music).unwrap();
    let mut folder = music.clone();
    for _ in 0..12 {
        folder.push("日本語-long-folder-name");
    }
    flac(&folder.join("曲.flac"), &[("TITLE", "日本語")]);
    #[cfg(unix)]
    std::os::unix::fs::symlink(&music, folder.join("cycle")).unwrap();
    let storage = Storage::open(dir.path().join("db")).unwrap();
    index::add_folder(&storage, &music).unwrap();
    scan(&storage);
    assert_eq!(songs(&storage).len(), 1);
}

#[test]
fn manual_recovery_absorbs_an_unreferenced_duplicate_and_keeps_old_album() {
    let dir = tempfile::tempdir().unwrap();
    let music = dir.path().join("Music");
    let path = music.join("Old/one.flac");
    flac(&path, &[("ALBUM", "Album"), ("ARTIST", "Artist")]);
    let storage = Storage::open(dir.path().join("db")).unwrap();
    index::add_folder(&storage, &music).unwrap();
    scan(&storage);
    let old = songs(&storage)[0];
    let album = album_ids(&storage)[0];
    let moved = music.join("New/one.flac");
    fs::create_dir_all(moved.parent().unwrap()).unwrap();
    fs::copy(&path, &moved).unwrap();
    fs::remove_file(&path).unwrap();
    scan(&storage);
    assert_eq!(songs(&storage).len(), 2);
    index::recover_file(&storage, old, &moved).unwrap();
    scan(&storage);
    assert_eq!(songs(&storage), vec![old]);
    assert_eq!(album_ids(&storage), vec![album]);
}

#[test]
fn explicit_split_and_metadata_change_reset_to_current_automatic_group() {
    let dir = tempfile::tempdir().unwrap();
    let music = dir.path().join("Music");
    let path = music.join("one.flac");
    flac(&path, &[("ALBUM", "Original")]);
    let storage = Storage::open(dir.path().join("db")).unwrap();
    index::add_folder(&storage, &music).unwrap();
    scan(&storage);
    let file = songs(&storage)[0];
    let manual = storage.split_local_group(vec![file]).unwrap();
    flac(&path, &[("ALBUM", "Edited")]);
    scan(&storage);
    assert_eq!(album_ids(&storage), vec![manual]);
    storage.reset_local_grouping(vec![file]).unwrap();
    let albums = storage
        .browse_collection(
            BrowseKind::Albums,
            BrowseQuery {
                limit: 10,
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(albums[0].title, "Edited");
}

#[test]
fn metadata_for_supported_audio_formats_does_not_decode_pcm() {
    let base = Path::new(env!("CARGO_MANIFEST_DIR")).join("../audio/tests/fixtures");
    for name in [
        "flac/s24_192000.flac",
        "wav/s24_48000.wav",
        "lossy/sine.mp3",
        "lossy/sine.m4a",
    ] {
        let result = metadata::read(&base.join(name)).unwrap();
        assert!(result.duration_ms > 0, "{name}");
        assert!(!result.format.is_empty());
    }
}

#[test]
fn cancel_between_files_does_not_reconcile_unseen_records() {
    let dir = tempfile::tempdir().unwrap();
    let music = dir.path().join("Music");
    for n in 0..3 {
        flac(&music.join(format!("{n}.flac")), &[("ALBUM", "One")]);
    }
    let storage = Storage::open(dir.path().join("db")).unwrap();
    index::add_folder(&storage, &music).unwrap();
    scan(&storage);
    let files = songs(&storage);
    fs::remove_file(music.join("0.flac")).unwrap();
    flac(&music.join("new.flac"), &[("TITLE", "New")]);
    let cancel = CancelToken::new();
    let result = index::scan(
        &storage,
        &storage.library_roots().unwrap()[0],
        &cancel,
        |progress| {
            if progress.visited > 0 {
                cancel.cancel();
            }
        },
    );
    assert!(matches!(result, Err(index::LibraryError::Cancelled)));
    for file in files {
        assert_eq!(
            storage.media_file(file).unwrap().unwrap().presence,
            Presence::Present
        );
    }
}

#[test]
fn rescans_and_moves_preserve_queue_links_and_custom_tags() {
    use parabolica_domain::ids::QueueEntryId;
    use parabolica_domain::queue::{QueueEntry, RepeatMode};
    use parabolica_domain::source::PlayableSource;
    use parabolica_storage::{queue::QueueCheckpoint, sources::LinkState};
    let dir = tempfile::tempdir().unwrap();
    let music = dir.path().join("Music");
    let path = music.join("Album/one.flac");
    flac(
        &path,
        &[("ALBUM", "Album"), ("RATING", "88"), ("CUSTOM", "Keep")],
    );
    let storage = Storage::open(dir.path().join("db")).unwrap();
    index::add_folder(&storage, &music).unwrap();
    scan(&storage);
    let file = songs(&storage)[0];
    let entry = QueueEntryId::generate();
    let queue = QueueCheckpoint {
        entries: vec![QueueEntry {
            id: entry,
            source: PlayableSource::LocalFile(file),
        }],
        current: Some(entry),
        position: Duration::from_secs(12),
        repeat: RepeatMode::All,
        shuffle: None,
    };
    storage.save_queue(queue.clone()).unwrap();
    let track = storage.create_track().unwrap();
    storage
        .link_source(
            track,
            PlayableSource::LocalFile(file),
            LinkState::Confirmed,
            None,
        )
        .unwrap();
    fs::rename(music.join("Album"), music.join("Moved")).unwrap();
    scan(&storage);
    assert_eq!(storage.load_queue().unwrap(), Some(queue));
    assert_eq!(songs(&storage), vec![file]);
    assert!(
        storage
            .media_file_tags(file)
            .unwrap()
            .iter()
            .any(|tag| tag.field == "RATING" && tag.value == "88")
    );
    assert_eq!(
        storage.track_sources(track).unwrap()[0].source,
        PlayableSource::LocalFile(file)
    );
}

#[test]
fn distinct_release_ids_in_one_folder_do_not_merge() {
    let dir = tempfile::tempdir().unwrap();
    let music = dir.path().join("Music");
    for edition in ["edition-a", "edition-b"] {
        flac(
            &music.join(format!("{edition}.flac")),
            &[
                ("ALBUM", "Same title"),
                ("COMPILATION", "1"),
                ("ARTIST", edition),
                ("MUSICBRAINZ_ALBUMID", edition),
            ],
        );
    }
    let storage = Storage::open(dir.path().join("db")).unwrap();
    index::add_folder(&storage, &music).unwrap();
    scan(&storage);
    assert_eq!(album_ids(&storage).len(), 2);
}

#[cfg(windows)]
#[test]
fn windows_junction_cycle_is_skipped() {
    let dir = tempfile::tempdir().unwrap();
    let music = dir.path().join("Music");
    flac(&music.join("Album/one.flac"), &[("ALBUM", "Album")]);
    let link = music.join("Album").join("cycle");
    let output = std::process::Command::new("cmd.exe")
        .args(["/C", "mklink", "/J"])
        .arg(&link)
        .arg(&music)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let storage = Storage::open(dir.path().join("db")).unwrap();
    index::add_folder(&storage, &music).unwrap();
    scan(&storage);
    let count = songs(&storage).len();
    std::fs::remove_dir(link).unwrap();
    assert_eq!(count, 1);
}

#[test]
fn a_replaced_volume_does_not_overwrite_the_stored_collection() {
    use parabolica_domain::files::VolumeId;
    let dir = tempfile::tempdir().unwrap();
    let music = dir.path().join("Music");
    flac(&music.join("one.flac"), &[]);
    let storage = Storage::open(dir.path().join("db")).unwrap();
    index::add_folder(&storage, &music).unwrap();
    scan(&storage);
    let files = songs(&storage);
    let mut root = storage.library_roots().unwrap().remove(0);
    root.volume = Some(VolumeId("different-volume".into()));
    assert!(index::scan(&storage, &root, &CancelToken::new(), |_| {}).is_err());
    assert_eq!(songs(&storage), files);
    assert_eq!(
        storage.library_roots().unwrap()[0].availability,
        RootAvailability::Offline
    );
}

#[test]
fn startup_cancels_interrupted_jobs_then_reconciles() {
    use parabolica_domain::jobs::{JobKind, JobState, Progress};
    let dir = tempfile::tempdir().unwrap();
    let music = dir.path().join("Music");
    flac(&music.join("one.flac"), &[]);
    let storage = Storage::open(dir.path().join("db")).unwrap();
    index::add_folder(&storage, &music).unwrap();
    let job = storage
        .create_job(JobKind::LibraryIndex, Vec::new())
        .unwrap();
    storage
        .transition_job(
            job,
            JobState::Running(Progress {
                done: 0,
                total: None,
            }),
        )
        .unwrap();
    let service = LibraryService::start(storage.clone());
    wait(|| {
        service.status().revision > 0 && !service.status().scanning && songs(&storage).len() == 1
    });
    assert_eq!(
        storage.job(job).unwrap().unwrap().state,
        JobState::Cancelled
    );
    assert!(storage.unfinished_jobs().unwrap().is_empty());
}

#[test]
fn an_interrupted_projection_update_is_retried_even_with_a_current_file_signature() {
    use parabolica_domain::files::FileLocation;
    use parabolica_storage::library::MediaFileRecord;
    let dir = tempfile::tempdir().unwrap();
    let music = dir.path().join("Music");
    let path = music.join("one.flac");
    flac(&path, &[("TITLE", "Before")]);
    let storage = Storage::open(dir.path().join("db")).unwrap();
    let root = index::add_folder(&storage, &music).unwrap();
    scan(&storage);
    let file = songs(&storage)[0];
    flac(&path, &[("TITLE", "After")]);
    let signature = index::signature_for_path(&path).unwrap();
    storage
        .record_media_file(MediaFileRecord {
            location: FileLocation {
                root,
                relative_path: "one.flac".into(),
            },
            signature,
            os_file_id: index::os_identity(&path),
            tags: metadata::read(&path).unwrap().tags,
        })
        .unwrap();
    assert!(!storage.indexed_files(root).unwrap()[0].projected);
    scan(&storage);
    assert_eq!(songs(&storage), vec![file]);
    let rows = storage
        .browse_collection(
            BrowseKind::Songs,
            BrowseQuery {
                text: "After".into(),
                limit: 10,
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(rows[0].title, "After");
}

fn mp3_with_custom_tags(path: &Path, title: &str) {
    let fixture =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../audio/tests/fixtures/lossy/sine.mp3");
    let original = fs::read(fixture).unwrap();
    let end = 10
        + original[6..10]
            .iter()
            .fold(0usize, |size, byte| (size << 7) | usize::from(*byte));
    let mut frames = Vec::new();
    for (key, value) in [
        ("TIT2", format!("\0{title}")),
        ("TXXX", "\0RATING\088".into()),
        ("TXXX", "\0REPLAYGAIN_TRACK_GAIN\0-7.5 dB".into()),
        ("TXXX", "\0CUSTOM_NOTE\0Keep this".into()),
    ] {
        frames.extend(key.bytes());
        frames.extend((value.len() as u32).to_be_bytes());
        frames.extend([0, 0]);
        frames.extend(value.bytes());
    }
    let size = frames.len();
    let mut data = b"ID3\x03\0\0".to_vec();
    data.extend([
        (size >> 21) as u8 & 127,
        (size >> 14) as u8 & 127,
        (size >> 7) as u8 & 127,
        size as u8 & 127,
    ]);
    data.extend(frames);
    data.extend(&original[end..]);
    fs::write(path, data).unwrap();
}

#[test]
fn non_flac_ratings_replaygain_and_custom_tags_survive_changed_file_rescans() {
    let dir = tempfile::tempdir().unwrap();
    let music = dir.path().join("Music");
    fs::create_dir(&music).unwrap();
    let path = music.join("one.mp3");
    mp3_with_custom_tags(&path, "Before");
    let storage = Storage::open(dir.path().join("db")).unwrap();
    index::add_folder(&storage, &music).unwrap();
    scan(&storage);
    let file = songs(&storage)[0];
    for title in ["Before", "After edited"] {
        mp3_with_custom_tags(&path, title);
        let expected = fs::read(&path).unwrap();
        scan(&storage);
        assert_eq!(songs(&storage), vec![file]);
        let tags = storage.media_file_tags(file).unwrap();
        for (field, value) in [
            ("TITLE", title),
            ("RATING", "88"),
            ("REPLAYGAIN_TRACK_GAIN", "-7.5 dB"),
            ("CUSTOM_NOTE", "Keep this"),
        ] {
            assert!(
                tags.iter()
                    .any(|tag| tag.field == field && tag.value == value),
                "missing {field} from {tags:?}"
            );
        }
        assert_eq!(fs::read(&path).unwrap(), expected);
    }
}

#[test]
fn destination_first_cross_root_reconciliation_preserves_file_album_and_queue_identity() {
    use parabolica_domain::ids::QueueEntryId;
    use parabolica_domain::queue::{QueueEntry, RepeatMode};
    use parabolica_domain::source::PlayableSource;
    use parabolica_storage::queue::QueueCheckpoint;
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("Source");
    let destination = dir.path().join("Destination");
    fs::create_dir(&destination).unwrap();
    flac(
        &source.join("Album/one.flac"),
        &[("ALBUM", "Album"), ("ARTIST", "Artist")],
    );
    let storage = Storage::open(dir.path().join("db")).unwrap();
    let destination_id = index::add_folder(&storage, &destination).unwrap();
    index::add_folder(&storage, &source).unwrap();
    scan(&storage);
    let file = songs(&storage)[0];
    let album = album_ids(&storage)[0];
    let entry = QueueEntryId::generate();
    let queue = QueueCheckpoint {
        entries: vec![QueueEntry {
            id: entry,
            source: PlayableSource::LocalFile(file),
        }],
        current: Some(entry),
        position: Duration::ZERO,
        repeat: RepeatMode::Off,
        shuffle: None,
    };
    storage.save_queue(queue.clone()).unwrap();
    fs::rename(source.join("Album"), destination.join("Album")).unwrap();
    let destination_root = storage
        .library_roots()
        .unwrap()
        .into_iter()
        .find(|root| root.id == destination_id)
        .unwrap();
    index::scan(&storage, &destination_root, &CancelToken::new(), |_| {}).unwrap();
    scan(&storage);
    assert_eq!(songs(&storage), vec![file]);
    assert_eq!(album_ids(&storage), vec![album]);
    assert_eq!(
        storage.media_file(file).unwrap().unwrap().location.root,
        destination_id
    );
    assert_eq!(storage.load_queue().unwrap(), Some(queue));
    assert!(storage.resolve_local_file(file).unwrap().is_file());
}

#[test]
fn failed_relocation_during_a_scan_does_not_disable_watcher_reconciliation() {
    let dir = tempfile::tempdir().unwrap();
    let music = dir.path().join("Music");
    for n in 0..100 {
        flac(&music.join(format!("{n}.flac")), &[("ALBUM", "One")]);
    }
    let storage = Storage::open(dir.path().join("db")).unwrap();
    let root = index::add_folder(&storage, &music).unwrap();
    let service = LibraryService::start(storage.clone());
    service.set_playing(true);
    wait(|| service.status().scanning && service.status().progress.visited > 0);
    service
        .send(Command::Relocate(root, dir.path().join("not-a-folder")))
        .unwrap();
    wait(|| service.status().command_error.is_some());
    wait(|| {
        let status = service.status();
        !status.scanning && status.progress.finished && !status.progress.cancelled
    });
    flac(&music.join("new.flac"), &[("TITLE", "New")]);
    wait(|| songs(&storage).len() == 101);
    assert!(service.status().command_error.is_some());
}

#[test]
fn pathological_wav_and_mp4_header_counts_and_total_metadata_are_rejected() {
    let dir = tempfile::tempdir().unwrap();
    for mp4 in [false, true] {
        let mut bytes = if mp4 {
            [8u32.to_be_bytes().as_slice(), b"ftyp"].concat()
        } else {
            b"RIFF\0\0\0\0WAVE".to_vec()
        };
        for _ in 0..4097 {
            if mp4 {
                bytes.extend(8u32.to_be_bytes());
                bytes.extend(b"free");
            } else {
                bytes.extend(b"JUNK");
                bytes.extend(0u32.to_le_bytes());
            }
        }
        let path = dir.path().join(if mp4 { "count.m4a" } else { "count.wav" });
        fs::write(&path, bytes).unwrap();
        assert!(
            metadata::read(&path)
                .unwrap_err()
                .to_string()
                .contains("4096")
        );
        let mut bytes = if mp4 {
            [8u32.to_be_bytes().as_slice(), b"ftyp"].concat()
        } else {
            b"RIFF\0\0\0\0WAVE".to_vec()
        };
        for _ in 0..9 {
            let size = 1024 * 1024u32;
            if mp4 {
                bytes.extend(size.to_be_bytes());
                bytes.extend(b"free");
                bytes.resize(bytes.len() + size as usize - 8, 0);
            } else {
                bytes.extend(b"LIST");
                bytes.extend(size.to_le_bytes());
                bytes.resize(bytes.len() + size as usize, 0);
            }
        }
        let path = dir.path().join(if mp4 { "total.m4a" } else { "total.wav" });
        fs::write(&path, bytes).unwrap();
        assert!(
            metadata::read(&path)
                .unwrap_err()
                .to_string()
                .contains("in total")
        );
    }
}

#[cfg(unix)]
#[test]
fn inaccessible_child_folder_is_reported_without_marking_its_files_missing() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let music = dir.path().join("Music");
    let denied = music.join("Denied");
    flac(&denied.join("one.flac"), &[]);
    let storage = Storage::open(dir.path().join("db")).unwrap();
    index::add_folder(&storage, &music).unwrap();
    scan(&storage);
    let file = songs(&storage)[0];
    fs::set_permissions(&denied, fs::Permissions::from_mode(0o0)).unwrap();
    let result = index::scan(
        &storage,
        &storage.library_roots().unwrap()[0],
        &CancelToken::new(),
        |_| {},
    );
    fs::set_permissions(&denied, fs::Permissions::from_mode(0o700)).unwrap();
    let progress = result.unwrap();
    assert!(!progress.finished);
    assert!(
        progress
            .failures
            .iter()
            .any(|failure| failure.contains("Denied") && failure.contains("Restore folder access"))
    );
    assert_eq!(
        storage.media_file(file).unwrap().unwrap().presence,
        Presence::Present
    );
}

#[cfg(unix)]
#[test]
fn a_file_digest_read_failure_keeps_indexing_and_defers_missing_reconciliation() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let music = dir.path().join("Music");
    let blocked = music.join("blocked.flac");
    flac(&blocked, &[("TITLE", "Before")]);
    flac(&music.join("gone.flac"), &[("TITLE", "Gone")]);
    let storage = Storage::open(dir.path().join("db")).unwrap();
    let root = index::add_folder(&storage, &music).unwrap();
    scan(&storage);
    let gone = storage
        .indexed_files(root)
        .unwrap()
        .into_iter()
        .find(|record| record.file.location.relative_path == Path::new("gone.flac"))
        .unwrap()
        .file
        .id;
    flac(&blocked, &[("TITLE", "Changed and unreadable")]);
    fs::set_permissions(&blocked, fs::Permissions::from_mode(0o000)).unwrap();
    fs::remove_file(music.join("gone.flac")).unwrap();
    for name in ["readable-a", "readable-b"] {
        flac(&music.join(format!("{name}.flac")), &[("TITLE", name)]);
    }
    let result = index::scan(
        &storage,
        &storage.library_roots().unwrap()[0],
        &CancelToken::new(),
        |_| {},
    );
    fs::set_permissions(&blocked, fs::Permissions::from_mode(0o600)).unwrap();
    let progress = result.unwrap();
    assert_eq!(progress.indexed, 2);
    assert!(!progress.finished);
    assert!(progress.failures.iter().any(|failure|failure.contains("blocked.flac")&&failure.contains("Can't read file:")));
    assert_eq!(
        storage.media_file(gone).unwrap().unwrap().presence,
        Presence::Present
    );
    scan(&storage);
    assert_eq!(
        storage.media_file(gone).unwrap().unwrap().presence,
        Presence::Missing
    );
}
