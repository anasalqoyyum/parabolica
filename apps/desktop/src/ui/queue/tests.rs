use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use gpui_kit::test::TestWindowExt as _;
use gpui_kit::{
    AnyWindowHandle, Bounds, Point, TestAppContext, WindowBounds, WindowOptions, px, size,
};
use parabolica_audio::engine::NoOutput;
use parabolica_domain::queue::RepeatMode;
use parabolica_storage::Storage;

use super::*;
use crate::controllers::queue::EnqueueMode;

pub(crate) fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../crates/audio/tests/fixtures")
        .join("flac/s16_44100.flac")
}

pub(crate) fn click(cx: &mut TestAppContext, window: AnyWindowHandle, id: &str) {
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click(SharedString::from(id.to_owned()), cx);
    })
    .unwrap();
    cx.run_until_parked();
}

pub(crate) fn settle(
    cx: &mut TestAppContext,
    queue: &QueueService,
    predicate: impl Fn(&QueueSnapshot) -> bool,
) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        cx.executor().advance_clock(Duration::from_millis(120));
        cx.run_until_parked();
        if predicate(&queue.snapshot()) {
            cx.executor().advance_clock(Duration::from_millis(120));
            cx.run_until_parked();
            return;
        }
        assert!(
            Instant::now() < deadline,
            "queue did not settle: {:?}",
            queue.snapshot()
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[gpui_kit::test]
fn queue_controls_keep_duplicate_ids_move_remove_and_bound_retries(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let storage = Storage::open(dir.path().join("app.db")).unwrap();
    let (queue, _) = QueueService::start(Some(storage.clone()), Box::new(NoOutput));
    let ids = queue.enqueue_files(
        vec![fixture(), fixture(), dir.path().join("missing.flac")],
        EnqueueMode::Now,
    );
    let window = cx.update(|cx| {
        gpui_kit::init(cx);
        gpui_kit::open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds {
                    origin: Point::default(),
                    size: size(px(512.), px(700.)),
                })),
                ..Default::default()
            },
            cx,
            |_, cx| cx.new(|cx| QueueView::new(queue.clone(), cx)),
        )
        .unwrap()
        .0
    });
    settle(cx, &queue, |state| {
        state.entries.len() == 3 && state.entries[2].details.unavailable_reason.is_some()
    });
    click(cx, window, "queue-down-0");
    settle(cx, &queue, |state| state.entries[0].entry.id == ids[1]);
    assert_eq!(queue.snapshot().current, Some(ids[0]));
    click(cx, window, "queue-up-1");
    settle(cx, &queue, |state| state.entries[0].entry.id == ids[0]);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        let source = window.within("queue-row-0").find("queue-source-label");
        assert_eq!(source.role(), Some(Role::Label));
        assert_eq!(source.value(), Some("s16_44100, Local file"));
        let unavailable = window.within("queue-row-2").find("queue-source-label");
        assert!(unavailable.value().unwrap().contains("missing"));
        assert!(unavailable.value().unwrap().contains("Unavailable:"));
        for id in [
            "queue-play-0",
            "queue-up-0",
            "queue-down-0",
            "queue-remove-0",
            "queue-retry-2",
        ] {
            let bounds = window.find(id).bounds();
            assert!(bounds.right() <= px(512.), "{id} is clipped: {bounds:?}");
            assert!(bounds.left() >= px(0.));
        }
    })
    .unwrap();
    click(cx, window, "queue-like-0");
    settle(cx, &queue, |state| state.likes.len() == 1);
    click(cx, window, "queue-remove-0");
    settle(cx, &queue, |state| {
        state.entries.len() == 2 && state.current == Some(ids[1])
    });
    click(cx, window, "queue-shuffle");
    settle(cx, &queue, |state| state.shuffle_seed.is_some());
    click(cx, window, "queue-repeat");
    settle(cx, &queue, |state| state.repeat == RepeatMode::All);
    click(cx, window, "queue-repeat");
    settle(cx, &queue, |state| state.repeat == RepeatMode::One);
    click(cx, window, "queue-repeat");
    settle(cx, &queue, |state| state.repeat == RepeatMode::Off);
    let missing = queue
        .snapshot()
        .entries
        .iter()
        .position(|entry| entry.entry.id == ids[2])
        .unwrap();
    for attempts in 1..=3 {
        click(cx, window, &format!("queue-retry-{missing}"));
        settle(cx, &queue, |state| {
            state
                .entries
                .iter()
                .any(|entry| entry.entry.id == ids[2] && entry.retry_attempts == attempts)
        });
    }
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(
            window
                .find(SharedString::from(format!("queue-retry-{missing}")))
                .disabled(),
            Some(true)
        );
    })
    .unwrap();
    click(cx, window, "queue-history");
    assert!(queue.snapshot().history.is_empty());
    click(cx, window, "queue-clear");
    settle(cx, &queue, |state| state.entries.is_empty());
    queue.shutdown();
    assert!(
        storage
            .load_session()
            .unwrap()
            .unwrap()
            .queue
            .entries
            .is_empty()
    );
}
