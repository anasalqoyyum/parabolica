use super::*;
use parabolica_audio::decode::Decoder;
use parabolica_replaygain::analyze::{self, Analyzer};
use std::path::PathBuf;

fn fixture_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/replaygain")
}
fn analyze_fixture(
    name: &str,
    method: PeakMethod,
) -> parabolica_domain::gain::analysis::TrackAnalysis {
    let path = fixture_root().join("fixtures").join(name);
    let mut decoder = Decoder::open(&path).unwrap();
    let info = decoder.info();
    if name == "mono-44100.flac" {
        assert_eq!(info.positioned_channel_mask(), Some(1));
    }
    let mut analyzer = Analyzer::new(
        info.format.rate.hz(),
        info.positioned_channel_mask().unwrap(),
        method,
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
    actual
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
        let actual = analyze_fixture(fields[0], PeakMethod::TruePeak);
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
        tracks.push(actual);
    }
    let album = analyze::album(&tracks[..3]).unwrap();
    let gain = album.gain_db().unwrap();
    let peak = album.peak;
    assert!((gain - (-18.0 + 8.104275638582)).abs() <= 0.1);
    assert!((peak - 0.400234699249).abs() < 0.00001);
    let reference = include_str!("../../../../../../tests/replaygain/references.tsv")
        .lines()
        .find_map(|line| line.strip_prefix("# album\tall\t"))
        .unwrap()
        .split('\t')
        .map(|value| value.parse::<f64>().unwrap())
        .collect::<Vec<_>>();
    let all = analyze::album(&tracks).unwrap();
    assert!((all.loudness_lufs.unwrap() - reference[0]).abs() <= 0.1);
    assert!((all.peak - reference[1]).abs() < 0.00001);
}

#[test]
fn pinned_foobar2000_track_and_album_gains_match_in_both_peak_modes() {
    for (method, label) in [
        (PeakMethod::Sample, "sample"),
        (PeakMethod::TruePeak, "true"),
    ] {
        let tracks = include_str!("../../../../../../tests/replaygain/references.tsv")
            .lines()
            .filter(|line| !line.starts_with('#'))
            .map(|line| {
                let name = line.split('\t').next().unwrap();
                (name, analyze_fixture(name, method))
            })
            .collect::<std::collections::BTreeMap<_, _>>();
        for line in std::fs::read_to_string(fixture_root().join("foobar2000.tsv"))
            .unwrap()
            .lines()
            .filter(|line| !line.starts_with('#'))
            .skip(1)
        {
            let fields = line.split('\t').collect::<Vec<_>>();
            if fields[2] != label {
                continue;
            }
            let actual = match fields[0] {
                "track" => tracks[fields[1]].gain_db(),
                "album" => {
                    let members = tracks
                        .iter()
                        .filter(|(name, _)| match fields[1] {
                            "all-nine" => true,
                            "mixed-rate-three" => matches!(
                                **name,
                                "loud-44100.flac" | "quiet-96000.flac" | "gated-48000.flac"
                            ),
                            group => panic!("Unknown reference group {group}"),
                        })
                        .map(|(_, track)| track.clone())
                        .collect::<Vec<_>>();
                    analyze::album(&members).unwrap().gain_db()
                }
                kind => panic!("Unknown reference kind {kind}"),
            };
            if fields[3] == "unavailable" {
                assert_eq!(actual, None, "{} {label}", fields[1]);
            } else {
                let expected = fields[3].parse::<f64>().unwrap();
                assert!(
                    (actual.unwrap() - expected).abs() <= 0.1,
                    "{} {label}: {actual:?} vs {expected}",
                    fields[1]
                );
            }
        }
    }
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

#[gpui_kit::test]
fn historical_resume_clears_unrelated_groups_before_starting_review(
    cx: &mut gpui_kit::TestAppContext,
) {
    use crate::commands::playback::PlaybackCommands;
    use gpui_kit::test::TestWindowExt as _;
    use parabolica_audio::engine::NoOutput;
    use parabolica_storage::replaygain::{
        GroupResult, MemberResult, ReviewedGroup, ScanMember, ScanRecord,
    };

    let previous = MediaFileId::generate();
    let missing = MediaFileId::generate();
    let history_id = JobId::generate();
    let group = |file| ReviewedGroup {
        name: "Reviewed selection".into(),
        warning: String::new(),
        members: vec![ScanMember {
            file,
            title: "Track".into(),
        }],
        albums: Vec::new(),
    };
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
    cx.update_window(window, |_, window, cx| {
        scanner.update(cx, |scanner, cx| {
            scanner.status.preview = Arc::new(vec![group(previous)]);
            scanner.accepted = true;
            scanner.selected = HashSet::from([previous]);
            scanner.showing = Some(history_id);
            scanner.status.history = Arc::new(vec![ScanRecord {
                id: history_id,
                created_at_ms: 0,
                analyzer: "test".into(),
                target_lufs: -18.0,
                method: PeakMethod::TruePeak,
                mode: ScanMode::Tracks,
                state: ScanState::Cancelled,
                saved: false,
                error: None,
                groups: vec![GroupResult {
                    reviewed: group(missing),
                    stale: false,
                    gain_db: None,
                    peak: None,
                    members: vec![MemberResult {
                        file: missing,
                        title: "Missing historical track".into(),
                        done: false,
                        detail: None,
                        measurement: None,
                        reused: false,
                    }],
                }],
            }]);
            cx.notify();
        });
        window.render_frame(cx);
        window.click("scanner-resume", cx);
        scanner.update(cx, |scanner, _| {
            assert_eq!(scanner.selected, HashSet::from([missing]));
            assert_eq!(scanner.mode, ScanMode::Tracks);
            assert_eq!(scanner.method, PeakMethod::TruePeak);
            assert!(scanner.status.preview.is_empty());
            assert!(!scanner.accepted);
            assert!(scanner.showing.is_none());
        });
        window.render_frame(cx);
        assert!(window.try_find("scanner-accept").is_none());
        assert!(window.try_find("scanner-start").is_none());
    })
    .unwrap();
}
