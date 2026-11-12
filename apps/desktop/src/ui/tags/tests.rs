use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use parabolica_domain::control::CancelToken;
use parabolica_domain::ids::ReleaseId;
use parabolica_domain::jobs::JobState;
use parabolica_domain::tags::{ArtworkDestination, FieldChange, FieldEdit, PictureType, TagField};
use parabolica_storage::Storage;
use parabolica_storage::collection::{BrowseKind, BrowseQuery, ItemId};
use parabolica_storage::tag_writes::{NewTagWrite, TagTargetState, TagWriteTarget};
use parabolica_tags::write::{self, FileEdit, NoHooks, Preparation};

use super::review::{self, ArtChoice, Choices, FieldChoice, Mode, TrackChoice};
use super::service::{Command, Status, TagService};

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../crates/audio/tests/fixtures")
        .join("flac/s16_44100.flac")
}

fn wait(service: &TagService, check: impl Fn(&Status) -> bool) -> Status {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let status = service.status();
        if !status.busy && check(&status) {
            return status;
        }
        assert!(
            Instant::now() < deadline,
            "Files & tags timed out: {status:?}"
        );
        thread::sleep(Duration::from_millis(10));
    }
}

fn send(service: &TagService, command: Command) -> Status {
    let before = service.status().revision;
    service.send(command).unwrap();
    wait(service, |s| s.revision > before)
}

/// A tagged copy of the audio fixture.
fn tagged(path: &Path, title: &str, disc: &str, track: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::copy(fixture(), path).unwrap();
    let reviewed = write::fingerprint(path).unwrap();
    let fields: Vec<FieldChange> = [
        (TagField::Album, "Old album"),
        (TagField::AlbumArtist, "Old artist"),
        (TagField::Title, title),
        (TagField::DiscNumber, disc),
        (TagField::TrackNumber, track),
        (TagField::ReplayGainTrackGain, "-4.20 dB"),
        (TagField::Custom("MY_NOTE".into()), "keep"),
    ]
    .into_iter()
    .map(|(field, value)| FieldChange {
        field,
        edit: FieldEdit::Replace(vec![value.into()]),
    })
    .collect();
    let Ok(Preparation::Ready(prepared)) = write::prepare(
        path,
        &reviewed,
        FileEdit {
            fields: &fields,
            pictures: &[],
        },
        &path.with_extension("tmp"),
        &path.with_extension("bak"),
        &NoHooks,
    ) else {
        panic!("fixture tagging failed");
    };
    let backup = prepared.backup().to_owned();
    prepared.commit(&NoHooks).unwrap();
    fs::remove_file(backup).unwrap();
}

struct Library {
    dir: tempfile::TempDir,
    storage: Storage,
    album: ReleaseId,
    files: Vec<PathBuf>,
}

fn library() -> Library {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("music");
    let files = vec![
        root.join("Album/CD1/01.flac"),
        root.join("Album/CD1/02.flac"),
        root.join("Album/CD2/01.flac"),
    ];
    tagged(&files[0], "One", "1", "1");
    tagged(&files[1], "Two", "1", "2");
    tagged(&files[2], "Three", "2", "1");
    fs::write(root.join("Album/CD1/folder.jpg"), b"old folder art").unwrap();
    let storage = Storage::open(dir.path().join("data/app.db")).unwrap();
    let albums = index(&storage, &root);
    assert_eq!(albums.len(), 1, "{albums:?}");
    let album = albums[0];
    // Library roots are canonical; on Windows that is a `\\?\` path.
    let files = files
        .into_iter()
        .map(|file| fs::canonicalize(file).unwrap())
        .collect();
    Library {
        dir,
        storage,
        album,
        files,
    }
}

fn index(storage: &Storage, root: &Path) -> Vec<ReleaseId> {
    let id = match storage.library_roots().unwrap().first() {
        Some(root) => root.id,
        None => parabolica_library::index::add_folder(storage, root).unwrap(),
    };
    let root = storage
        .library_roots()
        .unwrap()
        .into_iter()
        .find(|r| r.id == id)
        .unwrap();
    parabolica_library::index::scan(storage, &root, &CancelToken::new(), |_| {}).unwrap();
    let albums = storage
        .browse_collection(
            BrowseKind::Albums,
            BrowseQuery {
                limit: 10,
                ..Default::default()
            },
        )
        .unwrap();
    albums
        .into_iter()
        .map(|item| match item.id {
            ItemId::Album(album) => album,
            other => panic!("{other:?}"),
        })
        .collect()
}

/// A 1 × 1 grey-alpha PNG.
const PNG: [u8; 68] = [
    137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0, 0, 1, 8, 4, 0,
    0, 0, 181, 28, 12, 2, 0, 0, 0, 11, 73, 68, 65, 84, 120, 218, 99, 100, 96, 0, 0, 0, 6, 0, 2, 48,
    129, 208, 47, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130,
];

fn png(path: &Path) {
    fs::write(path, PNG).unwrap();
}

fn choices(status: &Status, edit: impl FnOnce(&mut Choices)) -> review::Plan {
    let review = status.review.clone().unwrap();
    let mut choices = Choices {
        fields: review::ALBUM_FIELDS
            .iter()
            .map(|field| FieldChoice {
                field: field.clone(),
                mode: Mode::Keep,
                values: Vec::new(),
            })
            .collect(),
        tracks: review
            .files
            .iter()
            .map(|f| TrackChoice {
                file: f.file,
                included: true,
                overrides: Vec::new(),
            })
            .collect(),
        art: Vec::new(),
    };
    edit(&mut choices);
    let images: HashMap<_, _> = status
        .staged
        .iter()
        .map(|s| (s.id, s.image.clone()))
        .collect();
    review::plan(&review, &choices, &images).unwrap()
}

fn set(choices: &mut Choices, field: TagField, mode: Mode, value: &str) {
    let choice = choices
        .fields
        .iter_mut()
        .find(|c| c.field == field)
        .unwrap();
    choice.mode = mode;
    choice.values = review::parse_values(value);
}

#[test]
fn album_fields_tracks_and_artwork_reach_files_and_the_library() {
    let lib = library();
    let service = TagService::start(lib.storage.clone());
    let status = send(&service, Command::Load(lib.album));
    let review = status.review.clone().unwrap();
    assert_eq!(review.files.len(), 3);
    assert!(
        review
            .files
            .iter()
            .all(|f| matches!(f.state, review::FileState::Ready { .. }))
    );
    let cd1 = lib.files[0].parent().unwrap();
    assert_eq!(
        review
            .folders
            .iter()
            .find(|f| f.path == cd1)
            .unwrap()
            .existing(PictureType::FrontCover),
        ["folder.jpg".to_owned()]
    );

    let cover = lib.dir.path().join("chosen.png");
    png(&cover);
    let status = send(
        &service,
        Command::Stage(cover.clone(), PictureType::FrontCover),
    );
    assert_eq!(status.staged.len(), 1);
    let not_image = lib.dir.path().join("notes.png");
    fs::write(&not_image, b"text").unwrap();
    let status = send(&service, Command::Stage(not_image, PictureType::BackCover));
    assert!(status.error.is_some());
    assert_eq!(status.staged.len(), 1);

    let excluded = lib.files[2].clone();
    let excluded_before = fs::read(&excluded).unwrap();
    let untouched_audio: Vec<Vec<u8>> = lib.files.iter().map(|f| audio(f)).collect();
    let plan = choices(&status, |c| {
        set(c, TagField::Album, Mode::Replace, "New album");
        set(c, TagField::Genre, Mode::FillEmpty, "Jazz; Soul");
        set(c, TagField::AlbumArtist, Mode::FillEmpty, "Ignored");
        c.tracks[0]
            .overrides
            .push((TagField::Title, vec!["First".into()]));
        c.tracks[2].included = false;
        c.art.push(ArtChoice {
            kind: PictureType::FrontCover,
            images: status.staged.iter().map(|s| s.id).collect(),
            destination: ArtworkDestination::Both,
            replace_embedded: false,
            replace_sidecars: true,
        });
    });
    assert_eq!(plan.targets.len(), 2);
    assert_eq!(plan.sidecars.len(), 1);
    assert!(plan.lines.iter().any(|l| l.change == "Title: One → First"));
    // Nothing touches files before the write is applied.
    assert!(lib.files[0].parent().unwrap().join("folder.jpg").exists());

    let status = send(&service, Command::Apply(Box::new(plan)));
    assert_eq!(status.error, None);
    let record = &status.history[0];
    assert_eq!(record.state, JobState::Completed);
    let first = write::read_tags(&lib.files[0]).unwrap();
    assert_eq!(first.values(&TagField::Album), ["New album"]);
    assert_eq!(first.values(&TagField::Title), ["First"]);
    assert_eq!(first.values(&TagField::Genre), ["Jazz", "Soul"]);
    assert_eq!(first.values(&TagField::AlbumArtist), ["Old artist"]);
    assert_eq!(first.values(&TagField::ReplayGainTrackGain), ["-4.20 dB"]);
    assert_eq!(first.values(&TagField::Custom("MY_NOTE".into())), ["keep"]);
    assert!(first.has_picture(PictureType::FrontCover));
    let second = write::read_tags(&lib.files[1]).unwrap();
    assert_eq!(second.values(&TagField::Title), ["Two"]);
    assert_eq!(fs::read(&excluded).unwrap(), excluded_before);
    for (file, before) in lib.files.iter().zip(&untouched_audio) {
        assert_eq!(&audio(file), before);
    }
    assert_eq!(
        fs::read(cd1.join("cover.png")).unwrap(),
        fs::read(&cover).unwrap()
    );
    assert!(!cd1.join("folder.jpg").exists());

    // The library picks up the new tags on its next pass. The written files now form their own
    // album; the excluded file keeps the old one, and the editor follows the written files.
    assert_eq!(index(&lib.storage, &lib.dir.path().join("music")).len(), 2);
    let written = record.targets[0].0.patch.target;
    let tags = lib.storage.media_file_tags(written).unwrap();
    assert!(
        tags.iter()
            .any(|t| t.field == "ALBUM" && t.value == "New album"),
        "{tags:?}"
    );
    let moved = lib.storage.album_of_file(written).unwrap().unwrap();
    assert_ne!(moved, lib.album);
    assert_eq!(lib.storage.album_edit_files(lib.album).unwrap().len(), 1);

    // Restore puts the original bytes back while the file still has the written content.
    let TagTargetState::Written { backup, .. } = &record.targets[0].1 else {
        panic!("{:?}", record.targets);
    };
    let original = fs::read(backup).unwrap();
    let status = send(
        &service,
        Command::Restore(record.id, record.targets[0].0.patch.target),
    );
    assert_eq!(status.error, None);
    assert_eq!(fs::read(&lib.files[0]).unwrap(), original);
    assert!(matches!(
        status.history[0].targets[0].1,
        TagTargetState::Restored { .. }
    ));
    let status = send(&service, Command::DiscardBackups(record.id));
    assert_eq!(status.error, None);
    assert!(!backup.exists());
    assert!(status.history[0].backups_discarded);
}

#[test]
fn playing_files_are_deferred_and_retried_without_duplicates() {
    let lib = library();
    let service = TagService::start(lib.storage.clone());
    let cover = lib.dir.path().join("chosen.png");
    png(&cover);
    send(&service, Command::Load(lib.album));
    let status = send(&service, Command::Stage(cover, PictureType::FrontCover));
    let plan = choices(&status, |c| {
        set(c, TagField::Date, Mode::Replace, "1999");
        c.art.push(ArtChoice {
            kind: PictureType::FrontCover,
            images: status.staged.iter().map(|s| s.id).collect(),
            destination: ArtworkDestination::Embed,
            replace_embedded: true,
            replace_sidecars: false,
        });
    });
    service.set_in_use(Some(lib.files[1].clone()));
    let playing_before = fs::read(&lib.files[1]).unwrap();
    let status = send(&service, Command::Apply(Box::new(plan)));
    let first = status.history[0].clone();
    assert_eq!(first.state, JobState::Incomplete);
    assert_eq!(
        first.targets[1].1,
        TagTargetState::Deferred {
            reason: "Playing in Parabolica".into()
        }
    );
    assert_eq!(fs::read(&lib.files[1]).unwrap(), playing_before);

    service.set_in_use(None);
    let status = send(&service, Command::Retry(first.id));
    let retry = &status.history[0];
    assert_eq!(retry.retry_of, Some(first.id));
    assert_eq!(retry.state, JobState::Completed);
    assert_eq!(retry.targets.len(), 1);
    for file in &lib.files {
        let tags = write::read_tags(file).unwrap();
        assert_eq!(tags.values(&TagField::Date), ["1999"]);
        assert_eq!(
            tags.pictures
                .iter()
                .filter(|p| p.kind == Some(PictureType::FrontCover))
                .count(),
            1
        );
    }
    // Nothing left to retry.
    assert!(service.send(Command::Retry(retry.id)).is_ok());
    let status = wait(&service, |s| s.error.is_some());
    assert_eq!(status.error.as_deref(), Some("Nothing to retry"));
}

#[test]
fn read_only_and_concurrently_edited_files_are_reported_per_file() {
    let lib = library();
    let service = TagService::start(lib.storage.clone());
    let status = send(&service, Command::Load(lib.album));
    let plan = choices(&status, |c| set(c, TagField::Label, Mode::Replace, "Label"));
    let readonly = &lib.files[0];
    let mut permissions = fs::metadata(readonly).unwrap().permissions();
    permissions.set_readonly(true);
    fs::set_permissions(readonly, permissions.clone()).unwrap();
    // Another tagger saves the second file after review.
    let edited = &lib.files[1];
    let mut bytes = fs::read(edited).unwrap();
    let at = bytes.windows(3).position(|w| w == b"Two").unwrap();
    bytes[at] = b'X';
    fs::write(edited, &bytes).unwrap();

    let status = send(&service, Command::Apply(Box::new(plan)));
    let record = &status.history[0];
    assert_eq!(record.state, JobState::Incomplete);
    assert_eq!(
        record.targets[0].1,
        TagTargetState::Failed {
            reason: "File is read-only".into()
        }
    );
    assert_eq!(record.targets[1].1, TagTargetState::Changed);
    assert!(matches!(
        record.targets[2].1,
        TagTargetState::Written { .. }
    ));
    assert_eq!(fs::read(edited).unwrap(), bytes);
    #[allow(clippy::permissions_set_readonly_false)]
    permissions.set_readonly(false);
    fs::set_permissions(readonly, permissions).unwrap();
    // Retry covers the read-only file only; the edited file needs a new review.
    let status = send(&service, Command::Retry(record.id));
    assert_eq!(status.history[0].targets.len(), 1);
    assert_eq!(status.history[0].state, JobState::Completed);
}

#[test]
fn startup_resolves_writes_interrupted_before_and_after_replacement() {
    let lib = library();
    let reviewed = write::fingerprint(&lib.files[0]).unwrap();
    let files = lib.storage.album_edit_files(lib.album).unwrap();
    let target = |index: usize| TagWriteTarget {
        patch: parabolica_domain::tags::TagPatch {
            target: files[index].file,
            reviewed: write::fingerprint(&lib.files[index]).unwrap(),
            fields: vec![FieldChange {
                field: TagField::Album,
                edit: FieldEdit::Replace(vec!["Crash".into()]),
            }],
            artwork: Vec::new(),
        },
        path: lib.files[index].clone(),
    };
    let job = lib
        .storage
        .create_tag_write(NewTagWrite {
            album: lib.album,
            retry_of: None,
            targets: vec![target(0), target(1)],
            images: Vec::new(),
            sidecars: Vec::new(),
        })
        .unwrap();
    lib.storage.start_tag_write(job).unwrap();
    // File 0: replacement recorded but not performed; a temporary file was left behind.
    let backup = lib.dir.path().join("data/tag-backups/manual.flac");
    fs::create_dir_all(backup.parent().unwrap()).unwrap();
    fs::copy(&lib.files[0], &backup).unwrap();
    let temp = parabolica_tags::job::temp_path(&lib.files[0], job);
    fs::write(&temp, b"partial").unwrap();
    lib.storage
        .record_tag_target(
            job,
            files[0].file,
            TagTargetState::Replacing {
                backup: backup.clone(),
                expected: parabolica_domain::files::FileSignature {
                    content_digest: Some(parabolica_domain::files::ContentDigest([9; 32])),
                    ..reviewed
                },
            },
        )
        .unwrap();

    let service = TagService::start(lib.storage.clone());
    let status = send(&service, Command::Load(lib.album));
    let record = &status.history[0];
    assert_eq!(record.id, job);
    assert!(matches!(record.state, JobState::Failed { .. }));
    assert_eq!(
        record.targets[0].1,
        TagTargetState::Failed {
            reason: "Interrupted before replacement; the file is unchanged".into()
        }
    );
    assert_eq!(
        record.targets[1].1,
        TagTargetState::Failed {
            reason: "Interrupted before writing; the file is unchanged".into()
        }
    );
    assert!(!temp.exists());
    assert!(!backup.exists());
    let status = send(&service, Command::Retry(job));
    assert_eq!(status.history[0].state, JobState::Completed);
    assert_eq!(
        write::read_tags(&lib.files[0])
            .unwrap()
            .values(&TagField::Album),
        ["Crash"]
    );
}

#[test]
fn reload_follows_files_regrouped_by_a_title_change() {
    let lib = library();
    let service = TagService::start(lib.storage.clone());
    let status = send(&service, Command::Load(lib.album));
    let plan = choices(&status, |c| {
        set(c, TagField::Album, Mode::Replace, "Renamed")
    });
    let status = send(&service, Command::Apply(Box::new(plan)));
    assert_eq!(status.history[0].state, JobState::Completed);
    let albums = index(&lib.storage, &lib.dir.path().join("music"));
    assert_eq!(albums.len(), 1);
    assert_ne!(albums[0], lib.album);
    let status = send(&service, Command::Load(lib.album));
    let review = status.review.unwrap();
    assert_eq!(review.album, albums[0]);
    assert_eq!(review.files.len(), 3);
    // The write's history moves with its files.
    assert_eq!(status.history.len(), 1);
}

fn audio(path: &Path) -> Vec<u8> {
    // Audio frames follow the last metadata block; compare from the first frame sync code after
    // the STREAMINFO-derived metadata length.
    let bytes = fs::read(path).unwrap();
    let mut offset = 4;
    loop {
        let header = &bytes[offset..offset + 4];
        let length = u32::from_be_bytes([0, header[1], header[2], header[3]]) as usize;
        offset += 4 + length;
        if header[0] & 0x80 != 0 {
            break;
        }
    }
    bytes[offset..].to_vec()
}

mod page {
    use gpui_kit::test::TestWindowExt as _;
    use gpui_kit::{
        AnyWindowHandle, AppContext as _, Bounds, Entity, Point, TestAppContext, WindowBounds,
        WindowOptions, px, size,
    };
    use parabolica_audio::engine::NoOutput;

    use super::*;
    use crate::commands::playback::PlaybackCommands;
    use crate::ui::library::LibraryView;
    use crate::ui::player::Player;
    use crate::ui::tags::FilesAndTags;

    fn open(cx: &mut TestAppContext, storage: Storage) -> (AnyWindowHandle, Entity<FilesAndTags>) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            crate::ui::library::init(Some(storage.clone()), cx);
            gpui_kit::open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds {
                        origin: Point::default(),
                        size: size(px(1400.), px(4000.)),
                    })),
                    ..Default::default()
                },
                cx,
                |_, cx| {
                    let player = cx.new(|cx| {
                        let (commands, events) = PlaybackCommands::with_storage_and_output(
                            Some(storage.clone()),
                            Box::new(NoOutput),
                        );
                        Player::with_commands(commands, events, cx)
                    });
                    let library = cx.new(|cx| LibraryView::new(player.clone(), cx));
                    cx.new(|cx| FilesAndTags::new(player, library, cx))
                },
            )
            .unwrap()
        })
    }

    fn settle(
        cx: &mut TestAppContext,
        view: &Entity<FilesAndTags>,
        predicate: impl Fn(&FilesAndTags) -> bool,
    ) {
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            cx.executor().advance_clock(Duration::from_millis(150));
            cx.run_until_parked();
            if cx.update(|cx| {
                let view = view.read(cx);
                !view.status.busy && predicate(view)
            }) {
                break;
            }
            assert!(Instant::now() < deadline, "Files & tags did not settle");
            thread::sleep(Duration::from_millis(10));
        }
    }

    fn click(cx: &mut TestAppContext, window: AnyWindowHandle, id: &str) {
        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            window.click(gpui_kit::SharedString::from(id.to_owned()), cx);
        })
        .unwrap();
        cx.run_until_parked();
    }

    #[gpui_kit::test]
    fn album_edit_review_write_and_partial_retry_through_the_page(cx: &mut TestAppContext) {
        let lib = library();
        let (window, view) = open(cx, lib.storage.clone());
        cx.update(|cx| view.update(cx, |view, cx| view.open_album(lib.album, cx)));
        settle(cx, &view, |view| view.status.review.is_some());

        click(cx, window, "tags-field-input-album");
        cx.update_window(window, |_, window, cx| window.input("Page album", cx))
            .unwrap();
        cx.run_until_parked();
        cx.update(|cx| assert_eq!(view.read(cx).modes[0], Mode::Replace));

        let readonly = lib.files[1].clone();
        let mut permissions = fs::metadata(&readonly).unwrap().permissions();
        permissions.set_readonly(true);
        fs::set_permissions(&readonly, permissions.clone()).unwrap();

        click(cx, window, "tags-review-button");
        cx.update(|cx| {
            let Some(Ok(plan)) = &view.read(cx).plan else {
                panic!("review failed");
            };
            assert_eq!(plan.targets.len(), 3);
        });
        click(cx, window, "tags-apply");
        settle(cx, &view, |view| !view.status.history.is_empty());
        let job = cx.update(|cx| {
            let record = view.read(cx).status.history[0].clone();
            assert_eq!(record.state, JobState::Incomplete);
            record.id
        });

        #[allow(clippy::permissions_set_readonly_false)]
        permissions.set_readonly(false);
        fs::set_permissions(&readonly, permissions).unwrap();
        click(cx, window, &format!("tags-retry-{job}"));
        settle(cx, &view, |view| view.status.history.len() == 2);
        cx.update(|cx| {
            let retry = &view.read(cx).status.history[0];
            assert_eq!(retry.retry_of, Some(job));
            assert_eq!(retry.state, JobState::Completed);
        });
        for file in &lib.files {
            assert_eq!(
                write::read_tags(file).unwrap().values(&TagField::Album),
                ["Page album"]
            );
        }
    }
}
