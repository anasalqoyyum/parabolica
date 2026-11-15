use super::*;

fn tone(rate: u32, seconds: f64, amplitude: f64) -> Vec<f64> {
    (0..(rate as f64 * seconds) as usize)
        .flat_map(|i| {
            let sample =
                amplitude * (std::f64::consts::TAU * 1000.0 * i as f64 / rate as f64).sin();
            [sample, sample]
        })
        .collect()
}
fn measure(rate: u32, pcm: &[f64], method: PeakMethod) -> TrackAnalysis {
    let mut a = Analyzer::new(rate, 3, method).unwrap();
    for block in pcm.chunks(2066) {
        a.add(block).unwrap();
    }
    a.finish().unwrap()
}
#[test]
fn track_and_mixed_rate_album_match_ebur128_multi_state() {
    let mut states = Vec::new();
    let mut tracks = Vec::new();
    for (rate, seconds, amplitude) in [(44100, 3.0, 0.4), (96000, 7.0, 0.02), (48000, 1.5, 0.1)] {
        let pcm = tone(rate, seconds, amplitude);
        let mut independent_state = EbuR128::new(2, rate, Mode::I).unwrap();
        independent_state.add_frames_f64(&pcm).unwrap();
        let track = measure(rate, &pcm, PeakMethod::Sample);
        assert!(
            (track.loudness_lufs.unwrap() - independent_state.loudness_global().unwrap()).abs()
                < 1e-9
        );
        states.push(independent_state);
        tracks.push(track);
    }
    let reference = EbuR128::loudness_global_multiple(states.iter()).unwrap();
    let album = album(&tracks).unwrap();
    let gain = album.gain_db().unwrap();
    let peak = album.peak;
    assert!((gain - (-18.0 - reference)).abs() < 1e-9);
    assert!((peak - tracks[0].peak).abs() < 1e-12);
    let wrong = tracks.iter().map(|t| t.gain_db().unwrap()).sum::<f64>() / 3.0;
    assert!((wrong - gain).abs() > 3.0);
}
#[test]
fn silence_short_audio_and_invalid_samples_are_explicit() {
    let silence = measure(48000, &vec![0.0; 96000], PeakMethod::Sample);
    assert_eq!(silence.loudness_lufs, None);
    assert_eq!(silence.peak, 0.0);
    let silent_album = album(&[silence]).unwrap();
    assert_eq!(silent_album.gain_db(), None);
    assert_eq!(silent_album.peak, 0.0);
    let short = measure(48000, &tone(48000, 0.2, 0.4), PeakMethod::Sample);
    assert_eq!(short.loudness_lufs, None);
    assert!(short.peak > 0.39);
    let short_album = album(&[short]).unwrap();
    assert_eq!(short_album.gain_db(), None);
    assert!(short_album.peak > 0.39);
    let mut a = Analyzer::new(48000, 3, PeakMethod::Sample).unwrap();
    assert!(a.add(&[f64::NAN, 0.0]).is_err());
    assert!(a.add(&[0.0]).is_err());
    assert!(a.finish().is_err());
    assert!(Analyzer::new(48000, (1 << 8) | 1, PeakMethod::Sample).is_err());
}
#[test]
fn lfe_is_excluded_from_loudness_but_kept_in_peaks() {
    let mut a = Analyzer::new(48000, 0x3f, PeakMethod::TruePeak).unwrap();
    let pcm = (0..48000)
        .flat_map(|_| [0.0, 0.0, 0.0, 0.5, 0.0, 0.0])
        .collect::<Vec<_>>();
    a.add(&pcm).unwrap();
    let result = a.finish().unwrap();
    assert_eq!(result.loudness_lufs, None);
    assert!(result.peak >= 0.5);
}
#[test]
fn mono_matches_identical_stereo_for_every_single_channel_position_without_changing_peaks() {
    let rate = 48000;
    let stereo = tone(rate, 0.8, 0.4);
    let mono = stereo.iter().step_by(2).copied().collect::<Vec<_>>();
    for method in [PeakMethod::Sample, PeakMethod::TruePeak] {
        let expected = measure(rate, &stereo, method);
        for bit in 0..64 {
            let mut analyzer = Analyzer::new(rate, 1 << bit, method).unwrap();
            for block in mono.chunks(1033) {
                analyzer.add(block).unwrap();
            }
            let actual = analyzer.finish().unwrap();
            assert!(actual.valid());
            assert_eq!(actual.channel_mask, 1 << bit);
            assert_eq!(actual.frames, expected.frames);
            assert!((actual.loudness_lufs.unwrap() - expected.loudness_lufs.unwrap()).abs() < 1e-9);
            assert_eq!(actual.sample_peak, expected.sample_peak);
            assert_eq!(actual.peak, expected.peak);
        }
    }
}
#[test]
fn relative_gate_excludes_quiet_passages() {
    let high = 10.0_f64.powf((-20.0 + 0.691) / 10.0);
    let low = 10.0_f64.powf((-45.0 + 0.691) / 10.0);
    assert!((integrated([high, high, low, low, 0.0]).unwrap() + 20.0).abs() < 1e-10);
}
#[test]
fn true_peak_detects_intersample_overs() {
    let pcm = (0..48000)
        .flat_map(|i| {
            let s = 0.9 * (std::f64::consts::TAU * 12000.0 * (i as f64 + 0.5) / 48000.0).sin();
            [s, s]
        })
        .collect::<Vec<_>>();
    let result = measure(48000, &pcm, PeakMethod::TruePeak);
    assert!(result.peak > result.sample_peak * 1.2);
}
