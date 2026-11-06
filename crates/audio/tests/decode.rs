//! Decoder checks against the fixtures in `tests/fixtures` (see `generate.py`).
//!
//! Lossless fixtures must decode to the exact PCM that libFLAC/libsndfile and FFmpeg produce,
//! recorded as MD5 sums in `references.json`.

use std::path::{Path, PathBuf};
use std::time::Duration;

use md5::{Digest, Md5};
use parabolica_audio::decode::{DecodeError, Decoder, EncoderDelay};
use parabolica_domain::format::{Codec, SourceSampleFormat, pcm_to_int_exact};
use serde_json::Value;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn references() -> serde_json::Map<String, Value> {
    let text = std::fs::read_to_string(fixture("references.json")).unwrap();
    serde_json::from_str::<Value>(&text)
        .unwrap()
        .as_object()
        .unwrap()
        .clone()
}

fn decode_all(decoder: &mut Decoder) -> Vec<f64> {
    let mut samples = Vec::new();
    while decoder.next_block(&mut samples).unwrap() > 0 {}
    samples
}

fn file_digest(path: &Path) -> Vec<u8> {
    Md5::digest(std::fs::read(path).unwrap()).to_vec()
}

#[test]
fn lossless_fixtures_match_reference_decoders_exactly() {
    let references = references();
    let lossless: Vec<_> = references
        .iter()
        .filter(|(_, reference)| reference.get("pcm_md5").is_some())
        .collect();
    assert_eq!(lossless.len(), 9);

    for (name, reference) in lossless {
        let bits = reference["bits"].as_u64().unwrap() as u8;
        let rate = reference["rate"].as_u64().unwrap() as u32;
        let frames = reference["frames"].as_u64().unwrap();
        let mut decoder = Decoder::open(&fixture(name)).unwrap();
        let info = decoder.info().clone();
        let expected_codec = if name.ends_with(".flac") {
            Codec::Flac
        } else {
            Codec::Wav
        };
        assert_eq!(info.format.codec, expected_codec, "{name}");
        assert_eq!(
            info.format.sample,
            Some(SourceSampleFormat::Int { bits }),
            "{name}"
        );
        assert_eq!(info.format.rate.hz(), rate, "{name}");
        assert_eq!(info.format.channels.get(), 2, "{name}");
        assert_eq!(info.frames, Some(frames), "{name}");

        let samples = decode_all(&mut decoder);
        assert_eq!(samples.len() as u64, frames * 2, "{name}");
        let mut pcm = Md5::new();
        for (index, sample) in samples.iter().enumerate() {
            let value = pcm_to_int_exact(*sample, bits).unwrap_or_else(|| {
                panic!("{name}: sample {index} is not an exact {bits}-bit value")
            });
            pcm.update(&value.to_le_bytes()[..usize::from(bits / 8)]);
        }
        assert_eq!(
            format!("{:x}", pcm.finalize()),
            reference["pcm_md5"].as_str().unwrap(),
            "{name}"
        );
    }
}

#[test]
fn seeking_lands_on_the_exact_frame() {
    for name in [
        "flac/s16_44100.flac",
        "flac/s24_96000.flac",
        "wav/s24_48000.wav",
    ] {
        let full = decode_all(&mut Decoder::open(&fixture(name)).unwrap());
        let total = full.len() as u64 / 2;
        for target in [0, 1, 4095, 4096, 4097, total / 2, total - 1, total] {
            let mut decoder = Decoder::open(&fixture(name)).unwrap();
            // Read some audio first so the seek starts from a decoded position.
            decoder.next_block(&mut Vec::new()).unwrap();
            assert_eq!(decoder.seek(target).unwrap(), target, "{name} @ {target}");
            assert_eq!(decoder.position(), target);
            let rest = decode_all(&mut decoder);
            assert!(
                rest == full[target as usize * 2..],
                "{name}: audio after seeking to {target} differs"
            );
        }
    }
}

#[test]
fn seeking_past_the_end_clamps_to_the_end() {
    let mut decoder = Decoder::open(&fixture("flac/s24_48000.flac")).unwrap();
    assert_eq!(decoder.seek(1_000_000).unwrap(), 9600);
    assert!(decode_all(&mut decoder).is_empty());
}

#[test]
fn corrupt_and_truncated_files_fail_without_changing_them() {
    let original = fixture("flac/s24_96000.flac");
    let original_digest = file_digest(&original);
    let bytes = std::fs::read(&original).unwrap();
    let dir = tempfile::tempdir().unwrap();

    let mut corrupt = bytes.clone();
    let middle = corrupt.len() / 2;
    for byte in &mut corrupt[middle..middle + 64] {
        *byte ^= 0x5a;
    }
    let corrupt_path = dir.path().join("corrupt.flac");
    std::fs::write(&corrupt_path, &corrupt).unwrap();

    let truncated_path = dir.path().join("truncated.flac");
    std::fs::write(&truncated_path, &bytes[..bytes.len() * 3 / 5]).unwrap();

    for path in [&corrupt_path, &truncated_path] {
        let before = file_digest(path);
        let mut decoder = Decoder::open(path).unwrap();
        let mut samples = Vec::new();
        let error = loop {
            match decoder.next_block(&mut samples) {
                Ok(0) => panic!("{} decoded without an error", path.display()),
                Ok(_) => {}
                Err(error) => break error,
            }
        };
        let expected_kind = if path == &corrupt_path {
            matches!(error, DecodeError::Corrupt { .. })
        } else {
            matches!(error, DecodeError::Truncated { .. })
        };
        assert!(expected_kind, "{}: {error:?}", path.display());
        assert_eq!(file_digest(path), before);
    }

    let truncated_error = {
        let mut decoder = Decoder::open(&truncated_path).unwrap();
        let mut samples = Vec::new();
        loop {
            if let Err(error) = decoder.next_block(&mut samples) {
                break error;
            }
        }
    };
    assert!(
        matches!(truncated_error, DecodeError::Truncated { expected: 19200, decoded } if decoded < 19200),
        "{truncated_error:?}"
    );
    assert_eq!(file_digest(&original), original_digest);
}

#[test]
fn unsupported_files_name_the_container_and_codec() {
    let cases = [
        (
            "unsupported/opus.ogg",
            "Opus audio in Ogg files is not supported",
        ),
        (
            "unsupported/vorbis.ogg",
            "Vorbis audio in Ogg files is not supported",
        ),
        (
            "unsupported/alac.m4a",
            "ALAC audio in MP4 files is not supported",
        ),
        ("unsupported/not-audio.flac", "not a recognized audio file"),
    ];
    for (name, message) in cases {
        let error = Decoder::open(&fixture(name)).unwrap_err();
        assert_eq!(error.to_string(), message, "{name}");
    }
    let missing = Decoder::open(&fixture("missing.flac")).unwrap_err();
    assert!(matches!(missing, DecodeError::Open(_)), "{missing:?}");
}

/// The 16-bit source the lossy fixtures were encoded from.
fn lossy_reference(frame: usize, channel: usize) -> f64 {
    let t = frame as f64 / 44_100.0;
    let cycles = match channel {
        0 => 200.0 * t + 900.0 * t * t,
        _ => 2000.0 * t - 900.0 * t * t,
    };
    let value = 0.5 * (2.0 * std::f64::consts::PI * cycles).sin();
    (value * 32768.0).round().clamp(-32768.0, 32767.0) / 32768.0
}

/// Signal-to-noise ratio in dB of `samples` (interleaved stereo starting at `first_frame` of the
/// source) against the reference, after skipping `lag` leading decoder frames.
fn snr_db(samples: &[f64], first_frame: usize, lag: usize, frames: usize) -> f64 {
    let (mut signal, mut noise) = (0.0, 0.0);
    for frame in 0..frames {
        for channel in 0..2 {
            let reference = lossy_reference(first_frame + frame, channel);
            let decoded = samples[(lag + frame) * 2 + channel];
            signal += reference * reference;
            noise += (decoded - reference).powi(2);
        }
    }
    10.0 * (signal / noise).log10()
}

/// The decoded sweep matches the source at `lag` and is clearly worse one frame either side.
fn assert_aligned(samples: &[f64], lag: usize) {
    let frames = 40_000;
    let aligned = snr_db(samples, 0, lag, frames);
    assert!(aligned > 25.0, "SNR {aligned:.1} dB at lag {lag}");
    for (first_frame, other_lag) in [(0, lag + 1), (1, lag)] {
        let snr = snr_db(samples, first_frame, other_lag, frames);
        assert!(aligned > snr + 10.0, "off by one frame: {snr:.1} dB");
    }
}

#[test]
fn mp3_decodes_gaplessly_and_seeks() {
    let references = references();
    let reference = &references["lossy/sine.mp3"];
    let mut decoder = Decoder::open(&fixture("lossy/sine.mp3")).unwrap();
    let info = decoder.info().clone();
    assert_eq!(info.container, "MP3");
    assert_eq!(info.format.codec, Codec::Mp3);
    assert_eq!(info.format.sample, None);
    assert_eq!(info.format.rate.hz(), 44_100);
    let delay = info.encoder_delay;
    assert!(delay.trimmed, "{delay:?}");
    assert!(
        delay.delay_frames.is_some_and(|frames| frames > 0),
        "{delay:?}"
    );
    assert!(delay.padding_frames.is_some(), "{delay:?}");

    let full = decode_all(&mut decoder);
    let frames = full.len() / 2;
    assert_eq!(frames as u64, reference["ffmpeg_frames"].as_u64().unwrap());
    assert_eq!(info.frames, Some(frames as u64));
    // Gapless trimming aligns frame 0 with the source.
    assert_aligned(&full, 0);

    let target = 22_050;
    assert_eq!(decoder.seek(target).unwrap(), target);
    let rest = decode_all(&mut decoder);
    assert_eq!(rest.len() / 2, frames - target as usize);
    // The bit reservoir makes the first frames after a seek approximate; check past them.
    assert!(snr_db(&rest[4096..], target as usize + 2048, 0, 10_000) > 25.0);
}

#[test]
fn aac_in_mp4_decodes_and_seeks() {
    let references = references();
    let reference = &references["lossy/sine.m4a"];
    let mut decoder = Decoder::open(&fixture("lossy/sine.m4a")).unwrap();
    let info = decoder.info().clone();
    assert_eq!(info.container, "MP4");
    assert_eq!(info.format.codec, Codec::Aac);
    // Symphonia's MP4 reader ignores the edit list, so the delay is unknown and the encoder's
    // 1024 priming frames stay in the output. FFmpeg removes them.
    assert_eq!(
        info.encoder_delay,
        EncoderDelay {
            delay_frames: None,
            padding_frames: None,
            trimmed: false
        }
    );
    let lag = 1024;

    let full = decode_all(&mut decoder);
    let frames = full.len() / 2;
    assert_eq!(
        frames as u64,
        reference["ffmpeg_frames"].as_u64().unwrap() + lag as u64
    );
    assert_aligned(&full, lag);

    let target = 22_050;
    assert_eq!(decoder.seek(target).unwrap(), target);
    let rest = decode_all(&mut decoder);
    assert_eq!(rest.len() / 2, frames - target as usize);
    assert!(snr_db(&rest[4096..], target as usize + 2048 - lag, 0, 10_000) > 25.0);
}

#[test]
fn duration_conversions_round_trip() {
    use parabolica_audio::decode::{duration_to_frames, frames_to_duration};
    use parabolica_domain::format::SampleRate;
    let rate = SampleRate::new(44_100).unwrap();
    assert_eq!(frames_to_duration(44_100, rate), Duration::from_secs(1));
    assert_eq!(duration_to_frames(Duration::from_secs(1), rate), 44_100);
    for frame in [0, 1, 441, 44_099, 44_101, 10_000_000] {
        assert_eq!(
            duration_to_frames(frames_to_duration(frame, rate), rate),
            frame
        );
    }
}

#[test]
fn damaged_lossy_frames_are_skipped() {
    let bytes = std::fs::read(fixture("lossy/sine.mp3")).unwrap();
    let mut damaged = bytes.clone();
    let middle = damaged.len() / 2;
    for byte in &mut damaged[middle..middle + 32] {
        *byte ^= 0x5a;
    }
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("damaged.mp3");
    std::fs::write(&path, &damaged).unwrap();
    let mut decoder = Decoder::open(&path).unwrap();
    let samples = decode_all(&mut decoder);
    assert!(samples.len() / 2 > 40_000, "{} frames", samples.len() / 2);
}
