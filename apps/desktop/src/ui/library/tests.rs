use std::path::Path;
use std::time::{Duration, Instant};

use gpui_kit::test::TestWindowExt as _;
use gpui_kit::{
    AnyWindowHandle, AppContext as _, Bounds, Entity, Point, TestAppContext, WindowBounds,
    WindowOptions, px, size,
};
use parabolica_audio::engine::NoOutput;
use parabolica_domain::files::RootAvailability;
use parabolica_storage::Storage;
use parabolica_storage::collection::{BrowseKind, BrowseQuery, ItemId};

use super::LibraryView;
use crate::commands::playback::PlaybackCommands;
use crate::ui::player::Player;

fn open(cx: &mut TestAppContext, storage: Storage) -> (AnyWindowHandle, Entity<LibraryView>) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        super::init(Some(storage), cx);
        gpui_kit::open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds {
                    origin: Point::default(),
                    size: size(px(1000.), px(700.)),
                })),
                ..Default::default()
            },
            cx,
            |_, cx| {
                let player = cx.new(|cx| {
                    let (commands, events) = PlaybackCommands::with_output(Box::new(NoOutput));
                    Player::with_commands(commands, events, cx)
                });
                cx.new(|cx| LibraryView::new(player, cx))
            },
        )
        .unwrap()
    })
}
fn click(cx: &mut TestAppContext, window: AnyWindowHandle, id: &str) {
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click(gpui_kit::SharedString::from(id.to_owned()), cx);
    })
    .unwrap();
    cx.run_until_parked();
}
fn settle(
    cx: &mut TestAppContext,
    view: &Entity<LibraryView>,
    predicate: impl Fn(&LibraryView, &gpui_kit::App) -> bool,
) {
    let deadline = Instant::now() + Duration::from_secs(12);
    loop {
        cx.executor().advance_clock(Duration::from_millis(250));
        cx.run_until_parked();
        if cx.update(|cx| predicate(view.read(cx), cx)) {
            break;
        }
        assert!(Instant::now() < deadline, "collection view did not settle");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[gpui_kit::test]
fn add_search_album_play_and_relocate_use_real_indexed_results(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let music = dir.path().join("音楽");
    std::fs::create_dir(&music).unwrap();
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../crates/audio/tests/fixtures")
        .join("flac/s16_44100.flac");
    std::fs::copy(&fixture, music.join("曲.flac")).unwrap();
    std::fs::copy(&fixture, music.join("二.flac")).unwrap();
    let storage = Storage::open(dir.path().join("library.db")).unwrap();
    let (window, view) = open(cx, storage.clone());
    settle(cx, &view, |view, _| !view.loading);
    click(cx, window, "add-music-folder");
    assert!(cx.did_prompt_for_paths());
    cx.simulate_path_prompt_response(|options| {
        assert!(options.directories);
        assert!(!options.files);
        Some(vec![music.clone()])
    });
    settle(cx, &view, |view, _| {
        view.items.len() == 1 && !view.loading && !view.status.scanning
    });
    let file = match storage
        .browse_collection(
            BrowseKind::Songs,
            BrowseQuery {
                text: "曲".into(),
                limit: 1,
                ..Default::default()
            },
        )
        .unwrap()[0]
        .id
    {
        ItemId::Song(file) => file,
        _ => panic!("song"),
    };
    click(cx, window, "collection-item-0");
    settle(cx, &view, |view, _| {
        view.kind == BrowseKind::Songs && !view.loading
    });
    cx.update(|cx| view.update(cx, |view, cx| view.search("曲".into(), cx)));
    settle(cx, &view, |view, _| view.items.len() == 1 && !view.loading);
    click(cx, window, "play-album");
    settle(cx, &view, |view, _| view.playback_request.is_some());
    // NoOutput verifies dispatch without claiming Windows playback.
    settle(cx, &view, |view, cx| {
        view.player.read(cx).indexed_entry_for_test().is_some()
    });
    cx.update(|cx| {
        assert_eq!(
            view.read(cx).player.read(cx).remaining_indexed_for_test(),
            1
        )
    });
    assert!(
        storage
            .resolve_local_file(file)
            .unwrap()
            .ends_with("曲.flac")
    );
    cx.update(|cx| view.update(cx, |view, cx| view.search("no such song".into(), cx)));
    settle(cx, &view, |view, _| view.items.is_empty() && !view.loading);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(window.find("collection-empty").value(), Some("No matches"));
    })
    .unwrap();
    cx.update(|cx| {
        view.update(cx, |view, cx| {
            view.search(String::new(), cx);
            view.show(BrowseKind::Folders, true, cx);
        })
    });
    settle(cx, &view, |view, _| view.roots.len() == 1 && !view.loading);
    let root = storage.library_roots().unwrap()[0].id;
    let moved = dir.path().join("Moved");
    std::fs::rename(&music, &moved).unwrap();
    storage
        .set_root_availability(root, RootAvailability::Offline)
        .unwrap();
    click(cx, window, &format!("folder-locate-{root}"));
    cx.simulate_path_prompt_response(|_| Some(vec![moved.clone()]));
    settle(cx, &view, |view, _| {
        view.roots
            .first()
            .is_some_and(|root| root.path == std::fs::canonicalize(&moved).unwrap())
    });
    assert!(storage.resolve_local_file(file).unwrap().is_file());
    click(cx, window, &format!("folder-remove-{root}"));
    assert_eq!(storage.library_roots().unwrap().len(), 1);
    click(cx, window, "confirm-remove-folder");
    settle(cx, &view, |view, _| {
        view.roots.is_empty() && !view.status.scanning
    });
    assert!(moved.join("曲.flac").exists());
}

#[gpui_kit::test]
fn grid_is_virtualized_and_search_replaces_stale_requests(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let music = dir.path().join("Music");
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../crates/audio/tests/fixtures")
        .join("flac/s16_44100.flac");
    for n in 0..100 {
        let folder = music.join(format!("Album-{n:03}"));
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::copy(&fixture, folder.join(format!("piece-{n:03}.flac"))).unwrap();
    }
    let storage = Storage::open(dir.path().join("db")).unwrap();
    parabolica_library::index::add_folder(&storage, &music).unwrap();
    let (window, view) = open(cx, storage);
    settle(cx, &view, |view, _| {
        view.items.len() == 96 && view.more && !view.loading && !view.status.scanning
    });
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("collection-item-0").is_some());
        assert!(window.try_find("collection-item-95").is_none());
    })
    .unwrap();
    click(cx, window, "collection-next");
    settle(cx, &view, |view, _| view.items.len() == 4 && !view.loading);
    cx.update(|cx| {
        view.update(cx, |view, cx| {
            view.search("missing".into(), cx);
            view.search("piece-099".into(), cx);
        })
    });
    settle(cx, &view, |view, _| view.items.len() == 1 && !view.loading);
}

#[gpui_kit::test]
fn indexing_error_paths_and_rescan_are_visible_in_collection_and_folder_settings(
    cx: &mut TestAppContext,
) {
    let dir = tempfile::tempdir().unwrap();
    let storage = Storage::open(dir.path().join("db")).unwrap();
    let (window, view) = open(cx, storage);
    settle(cx, &view, |view, _| {
        !view.loading && view.status.revision > 0 && !view.status.scanning
    });
    let failure = "Music/Denied: Permission denied. Restore folder access, then rescan.";
    for settings in [false, true] {
        cx.update(|cx| {
            view.update(cx, |view, cx| {
                view.folders_settings = settings;
                view.status.failures = vec![failure.into()];
                cx.notify();
            })
        });
        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            assert_eq!(window.find("index-failure-details").label(), Some(failure));
            assert_ne!(window.find("index-failures-rescan").disabled(), Some(true));
        })
        .unwrap();
    }
    click(cx, window, "index-failures-rescan");
    settle(cx, &view, |view, _| {
        view.status.failures.is_empty() && !view.status.scanning
    });
}
