use std::path::{Path, PathBuf};
use std::time::Duration;

use gpui_kit::test::{TestQueryExt as _, TestWindowExt as _};
use gpui_kit::{
    AnyWindowHandle, Bounds, Entity, Point, TestAppContext, WindowBounds, WindowOptions, px, size,
};
use parabolica_audio::engine::NoOutput;
use parabolica_domain::format::{Codec, SampleRate};

use super::*;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../crates/audio/tests/fixtures")
        .join(name)
}

fn open_player(cx: &mut TestAppContext, width: f32) -> (AnyWindowHandle, Entity<Player>) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds {
                origin: Point::default(),
                size: size(px(width), px(480.)),
            })),
            ..Default::default()
        };
        gpui_kit::open_window(options, cx, |_, cx| {
            cx.new(|cx| {
                let (commands, events) = PlaybackCommands::with_output(Box::new(NoOutput));
                Player::with_commands(commands, events, cx)
            })
        })
        .expect("open the player window")
    })
}

#[gpui_kit::test]
fn a_failed_output_shows_an_error_and_the_player_stays_usable(cx: &mut TestAppContext) {
    let (window, player) = open_player(cx, 1280.);
    cx.update(|cx| {
        player.update(cx, |player, _| {
            player.commands.open_file(fixture("flac/s24_48000.flac"));
        })
    });

    let expected = "Can't play s24_48000.flac: audio output is not available on this platform yet";
    let mut label = None;
    for _ in 0..200 {
        // The engine runs on its own threads; give it real time, then run the poll timer.
        std::thread::sleep(Duration::from_millis(5));
        cx.executor().advance_clock(FRAME);
        cx.run_until_parked();
        label = cx
            .update_window(window, |_, window, cx| {
                window.render_frame(cx);
                window
                    .try_find("player-error")
                    .and_then(|error| error.label().map(str::to_owned))
            })
            .unwrap();
        if label.is_some() {
            break;
        }
    }
    assert_eq!(label.as_deref(), Some(expected));
    cx.update_window(window, |_, window, _| {
        assert_eq!(window.find("now-playing").value(), Some("Nothing playing"));
        assert_ne!(window.find("player-play").disabled(), Some(true));
        assert_ne!(window.find("open-file").disabled(), Some(true));
    })
    .unwrap();
}

#[test]
fn labels_report_source_format_and_time() {
    let mut info = parabolica_audio::decode::Decoder::open(&fixture("flac/s24_96000.flac"))
        .unwrap()
        .info()
        .clone();
    assert_eq!(format_label(&info), "FLAC · 24-bit · 96 kHz");
    info.format.codec = Codec::Mp3;
    info.format.sample = None;
    info.format.rate = SampleRate::new(44_100).unwrap();
    assert_eq!(format_label(&info), "MP3 · 44.1 kHz");

    assert_eq!(clock(Duration::from_secs(84)), "1:24");
    assert_eq!(clock(Duration::from_millis(59_999)), "0:59");
    assert_eq!(clock(Duration::from_secs(3_725)), "1:02:05");
}

#[gpui_kit::test]
fn controls_stay_apart_at_the_minimum_window_width(cx: &mut TestAppContext) {
    // `main.rs` sets a 640 px minimum window width.
    let (window, _player) = open_player(cx, 640.);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        let transport = window.find("player-transport").bounds();
        assert!(window.find("open-file").bounds().right() <= transport.left());
        let volume = window.find_by_label("Volume").bounds();
        assert!(transport.right() <= volume.left());
        assert!(volume.right() <= px(640.));
        assert!(
            volume.size.width >= px(96.),
            "volume slider is {:?} wide",
            volume.size.width
        );
    })
    .unwrap();
}
