use gpui_kit::test::TestWindowExt as _;
use gpui_kit::{Bounds, Point, TestAppContext, WindowBounds, WindowOptions, px, size};
use parabolica_audio::engine::NoOutput;
use parabolica_storage::Storage;

use super::*;
use crate::ui::queue::tests::{click, fixture, settle};

#[gpui_kit::test]
fn auto_selected_playlists_require_fresh_delete_confirmation(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let storage = Storage::open(dir.path().join("app.db")).unwrap();
    let (queue, _) = QueueService::start(Some(storage.clone()), Box::new(NoOutput));
    let (window, view) = cx.update(|cx| {
        gpui_kit::init(cx);
        gpui_kit::open_window(WindowOptions::default(), cx, |window, cx| {
            cx.new(|cx| PlaylistsView::new(queue.clone(), window, cx))
        })
        .unwrap()
    });
    queue.create_playlist("Original".into());
    settle(cx, &queue, |state| state.playlists.len() == 1);
    let original = queue.snapshot().playlists[0].id;
    for (index, import) in [false, true].into_iter().enumerate() {
        click(cx, window, &format!("playlist-{original}"));
        click(cx, window, "playlist-delete");
        if import {
            let path = dir.path().join("imported.m3u8");
            std::fs::write(&path, "#EXTM3U\nmissing.flac\n").unwrap();
            click(cx, window, "playlist-import");
            assert!(cx.did_prompt_for_paths());
            cx.simulate_path_prompt_response(|_| Some(vec![path]));
        } else {
            cx.update_window(window, |_, window, cx| {
                view.update(cx, |view, cx| {
                    view.name
                        .update(cx, |name, cx| name.set_value("Created", window, cx));
                });
            })
            .unwrap();
            click(cx, window, "playlist-create");
        }
        let count = index + 2;
        settle(cx, &queue, |state| state.playlists.len() == count);
        let selected = queue.snapshot().last_created_playlist.unwrap();
        cx.update(|cx| {
            assert_eq!(view.read(cx).selected, Some(selected));
            assert!(!view.read(cx).confirm_delete);
        });
        click(cx, window, "playlist-delete");
        let revision = queue.snapshot().revision;
        settle(cx, &queue, |state| state.revision > revision);
        assert_eq!(queue.snapshot().playlists.len(), count);
        assert!(storage.playlist(original).is_ok());
        assert!(storage.playlist(selected).is_ok());
        cx.update(|cx| assert!(view.read(cx).confirm_delete));
        click(cx, window, "playlist-cancel-delete");
    }
    queue.shutdown();
}

#[gpui_kit::test]
fn playlist_controls_create_append_reorder_rename_import_export_and_delete(
    cx: &mut TestAppContext,
) {
    let dir = tempfile::tempdir().unwrap();
    let storage = Storage::open(dir.path().join("app.db")).unwrap();
    let (queue, _) = QueueService::start(Some(storage.clone()), Box::new(NoOutput));
    queue.enqueue_files(vec![fixture(), fixture()], EnqueueMode::Later);
    let (window, view) = cx.update(|cx| {
        gpui_kit::init(cx);
        gpui_kit::open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds {
                    origin: Point::default(),
                    size: size(px(640.), px(800.)),
                })),
                ..Default::default()
            },
            cx,
            |window, cx| cx.new(|cx| PlaylistsView::new(queue.clone(), window, cx)),
        )
        .unwrap()
    });
    settle(cx, &queue, |state| state.ready && state.entries.len() == 2);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click("playlist-name", cx);
        window.input("夜の音楽", cx);
    })
    .unwrap();
    click(cx, window, "playlist-create");
    settle(cx, &queue, |state| state.playlists.len() == 1);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(window.find("playlist-likes").selected(), Some(false));
        let id = queue.snapshot().playlists[0].id;
        assert_eq!(
            window
                .find(SharedString::from(format!("playlist-{id}")))
                .selected(),
            Some(true)
        );
    })
    .unwrap();
    click(cx, window, "playlist-add-queue");
    settle(cx, &queue, |state| state.playlists[0].entries.len() == 2);
    let id = queue.snapshot().playlists[0].id;
    let first = queue.snapshot().playlists[0].entries[0].id;
    click(cx, window, "playlist-down-0");
    settle(cx, &queue, |state| {
        state.playlists[0].entries[1].id == first
    });
    click(cx, window, "playlist-like-0");
    settle(cx, &queue, |state| state.likes.len() == 1);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.name
                .update(cx, |name, cx| name.set_value("Renamed", window, cx))
        });
    })
    .unwrap();
    click(cx, window, "playlist-rename");
    settle(cx, &queue, |state| state.playlists[0].name == "Renamed");
    click(cx, window, "playlist-next");
    settle(cx, &queue, |state| state.entries.len() == 4);
    click(cx, window, "playlist-later");
    settle(cx, &queue, |state| state.entries.len() == 6);
    click(cx, window, "playlist-export");
    assert!(cx.did_prompt_for_new_path());
    let exported = dir.path().join("exported.m3u8");
    cx.simulate_new_path_selection(|_| Some(exported.clone()));
    settle(cx, &queue, |state| state.last_export.is_some());
    assert_eq!(
        std::fs::read_to_string(exported).unwrap().lines().count(),
        3
    );
    let import = dir.path().join("import.m3u8");
    std::fs::write(&import, "#EXTM3U\nmissing.flac\n").unwrap();
    click(cx, window, "playlist-import");
    assert!(cx.did_prompt_for_paths());
    cx.simulate_path_prompt_response(|_| Some(vec![import.clone()]));
    settle(cx, &queue, |state| state.playlists.len() == 2);
    assert!(
        queue.snapshot().playlists[1].entries[0]
            .details
            .unavailable_reason
            .is_some()
    );
    click(cx, window, "playlist-delete");
    assert_eq!(queue.snapshot().playlists.len(), 2);
    click(cx, window, "playlist-delete");
    settle(cx, &queue, |state| state.playlists.len() == 1);
    assert_eq!(queue.snapshot().playlists[0].id, id);
    queue.shutdown();
    assert_eq!(storage.local_likes().unwrap().len(), 1);
}
