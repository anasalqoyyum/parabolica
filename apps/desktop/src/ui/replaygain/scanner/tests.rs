use super::*;
use parabolica_audio::decode::Decoder;
use parabolica_replaygain::analyze::{self, Analyzer};
use std::path::PathBuf;

fn fixture_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/replaygain")
}
#[test]
fn pinned_fixtures_match_independent_c_libebur128() {
    let root = fixture_root();
    let mut tracks = Vec::new();
    for line in include_str!("../../../../../../tests/replaygain/references.tsv")
        .lines()
        .filter(|l| !l.starts_with('#'))
    {
        let fields = line.split('\t').collect::<Vec<_>>();
        let path = root.join("fixtures").join(fields[0]);
        let signature = analyze::fingerprint(&path, || Ok(())).unwrap();
        let digest = signature
            .content_digest
            .unwrap()
            .0
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>();
        assert_eq!(digest, fields[5]);
        let mut decoder = Decoder::open(&path).unwrap();
        let info = decoder.info();
        let mut analyzer = Analyzer::new(
            info.format.rate.hz(),
            info.positioned_channel_mask().unwrap(),
            PeakMethod::TruePeak,
        )
        .unwrap();
        let mut pcm = Vec::new();
        loop {
            pcm.clear();
            if decoder.next_block(&mut pcm).unwrap() == 0 {
                break;
            }
            analyzer.add(&pcm).unwrap();
        }
        let actual = analyzer.finish().unwrap();
        assert_eq!(actual.frames, decoder.position());
        let expected = fields[2].parse::<f64>().unwrap();
        if expected.is_finite() {
            assert!(
                (actual.loudness_lufs.unwrap() - expected).abs() <= 0.1,
                "{}: {:?} vs {}",
                fields[0],
                actual.loudness_lufs,
                expected
            );
        } else {
            assert_eq!(actual.loudness_lufs, None);
        }
        assert!((actual.sample_peak - fields[3].parse::<f64>().unwrap()).abs() < 1e-10);
        assert!(
            (actual.peak - fields[4].parse::<f64>().unwrap()).abs() < 0.00001,
            "{}: peak {} vs {}",
            fields[0],
            actual.peak,
            fields[4]
        );
        if tracks.len() < 3 {
            tracks.push(actual);
        }
    }
    let album = analyze::album(&tracks).unwrap();
    let gain = album.gain_db().unwrap();
    let peak = album.peak;
    assert!((gain - (-18.0 + 8.104275638582)).abs() <= 0.1);
    assert!((peak - 0.400234699249).abs() < 0.00001);
}

#[gpui_kit::test]
fn cancellation_is_only_offered_for_operations_that_observe_it(cx: &mut gpui_kit::TestAppContext) {
    use crate::commands::playback::PlaybackCommands;
    use gpui_kit::test::TestWindowExt as _;
    use parabolica_audio::engine::NoOutput;

    let (window, scanner) = cx.update(|cx| {
        gpui_kit::init(cx);
        gpui_kit::open_window(gpui_kit::WindowOptions::default(), cx, |_, cx| {
            let player = cx.new(|cx| {
                let (commands, events) = PlaybackCommands::with_output(Box::new(NoOutput));
                Player::with_commands(commands, events, cx)
            });
            cx.new(|cx| Scanner::new(player, cx))
        })
        .unwrap()
    });
    for (operation, cancellable) in [
        ("Refreshing library", false),
        ("Reviewing groups", false),
        ("Scanning", true),
        ("Validating files", true),
    ] {
        cx.update_window(window, |_, window, cx| {
            scanner.update(cx, |scanner, cx| {
                scanner.status.busy = true;
                scanner.status.operation = operation.into();
                scanner.status.cancellable = cancellable;
                cx.notify();
            });
            window.render_frame(cx);
            assert_eq!(
                window.try_find("scanner-cancel").is_some(),
                cancellable,
                "{operation}"
            );
        })
        .unwrap();
    }
}
