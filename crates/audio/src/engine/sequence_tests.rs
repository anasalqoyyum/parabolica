use super::*;

pub(super) fn write_wav(path: &Path, rate: u32, channels: u16, samples: &[i16]) {
    let bytes = samples.len() as u32 * 2;
    let mut data = Vec::new();
    data.extend_from_slice(b"RIFF");
    data.extend_from_slice(&(36 + bytes).to_le_bytes());
    data.extend_from_slice(b"WAVEfmt ");
    data.extend_from_slice(&16u32.to_le_bytes());
    data.extend_from_slice(&1u16.to_le_bytes());
    data.extend_from_slice(&channels.to_le_bytes());
    data.extend_from_slice(&rate.to_le_bytes());
    data.extend_from_slice(&(rate * u32::from(channels) * 2).to_le_bytes());
    data.extend_from_slice(&(channels * 2).to_le_bytes());
    data.extend_from_slice(&16u16.to_le_bytes());
    data.extend_from_slice(b"data");
    data.extend_from_slice(&bytes.to_le_bytes());
    for sample in samples {
        data.extend_from_slice(&sample.to_le_bytes());
    }
    std::fs::write(path, data).unwrap();
}

fn crc8(bytes: &[u8]) -> u8 {
    bytes.iter().fold(0, |mut crc, byte| {
        crc ^= byte;
        for _ in 0..8 {
            crc = (crc << 1) ^ if crc & 0x80 != 0 { 0x07 } else { 0 };
        }
        crc
    })
}

fn crc16(bytes: &[u8]) -> u16 {
    bytes.iter().fold(0, |mut crc, byte| {
        crc ^= u16::from(*byte) << 8;
        for _ in 0..8 {
            crc = (crc << 1) ^ if crc & 0x8000 != 0 { 0x8005 } else { 0 };
        }
        crc
    })
}

fn utf8_number(value: usize, bytes: &mut Vec<u8>) {
    if value < 128 {
        bytes.push(value as u8);
        return;
    }
    let count = (2..=6).find(|n| value < 1usize << (5 * n + 1)).unwrap();
    bytes.push((0xffu8 << (8 - count)) | (value >> (6 * (count - 1))) as u8);
    for index in (0..count - 1).rev() {
        bytes.push(0x80 | ((value >> (6 * index)) & 0x3f) as u8);
    }
}

// Verbatim FLAC subframes keep the fixture independent of an encoder executable in CI.
fn write_flac(path: &Path, rate: u32, channels: u16, samples: &[i16]) {
    let mut data = b"fLaC\x80\0\0\x22".to_vec();
    data.extend_from_slice(&16u16.to_be_bytes());
    data.extend_from_slice(&256u16.to_be_bytes());
    data.extend_from_slice(&[0; 6]);
    let frames = samples.len() / usize::from(channels);
    let info =
        (u64::from(rate) << 44) | (u64::from(channels - 1) << 41) | (15 << 36) | frames as u64;
    data.extend_from_slice(&info.to_be_bytes());
    data.extend_from_slice(&[0; 16]);
    for (index, block) in samples.chunks(256 * usize::from(channels)).enumerate() {
        let block_frames = block.len() / usize::from(channels);
        let mut encoded = vec![0xff, 0xf9, 0x60, ((channels - 1) << 4) as u8 | 0x08];
        utf8_number(index * 256, &mut encoded);
        encoded.push((block_frames - 1) as u8);
        encoded.push(crc8(&encoded));
        for channel in 0..usize::from(channels) {
            encoded.push(0x02);
            for sample in block.iter().skip(channel).step_by(usize::from(channels)) {
                encoded.extend_from_slice(&sample.to_be_bytes());
            }
        }
        encoded.extend_from_slice(&crc16(&encoded).to_be_bytes());
        data.extend_from_slice(&encoded);
    }
    std::fs::write(path, data).unwrap();
}

fn samples(frames: usize, channels: u16) -> Vec<i16> {
    (0..frames)
        .flat_map(|frame| {
            (0..channels).map(move |channel| {
                let value = (1000 + frame * 37 % 20000) as i16;
                if channel == 0 { value } else { -value }
            })
        })
        .collect()
}

pub(super) fn sequence(
    paths: &[PathBuf],
    repeat: RepeatMode,
) -> (PlaybackSequence, Vec<QueueEntryId>) {
    let entries: Vec<_> = paths
        .iter()
        .map(|path| {
            (
                QueueEntryId::generate(),
                ResolvedSource::local_file(MediaFileId::generate(), path.clone()),
            )
        })
        .collect();
    let ids = entries.iter().map(|(id, _)| *id).collect();
    (PlaybackSequence::new(entries, repeat).unwrap(), ids)
}

impl Capture {
    fn rendered_epoch(&self, epoch: u64) -> Vec<f32> {
        let captured = self.captured.lock().unwrap();
        let mut output = Vec::new();
        let mut started = false;
        for (block_epoch, samples, consumed, outcome) in &captured.callbacks {
            if *block_epoch != epoch {
                continue;
            }
            started |= *consumed > 0;
            if !started {
                continue;
            }
            if *outcome == RenderOutcome::Finished {
                output.extend_from_slice(&samples[..consumed * 2]);
                return output;
            }
            output.extend_from_slice(samples);
        }
        output
    }
}

fn render_sequence(paths: &[PathBuf], rate: u32) -> (Vec<f32>, Harness, Vec<QueueEntryId>) {
    let mut harness = Harness::new(rate);
    let (sequence, ids) = sequence(paths, RepeatMode::Off);
    harness
        .engine
        .start_sequence(sequence, ids[0], Duration::ZERO, false)
        .unwrap();
    let ended = harness.wait("sequence ended", |snapshot| {
        snapshot.playback.current == ids.last().copied()
            && snapshot.playback.transport == TransportState::Stopped
    });
    assert!(ended.failure.is_none(), "{:?}", ended.failure);
    assert_eq!(ended.underruns, 0);
    let rendered = harness
        .capture
        .rendered_epoch(ended.playback.generation.get());
    assert_eq!(*harness.capture.opens.lock().unwrap(), 1);
    (rendered, harness, ids)
}

#[test]
fn split_wav_and_flac_albums_match_unsplit_callbacks_exactly_with_resampling_and_pressure() {
    for extension in ["wav", "flac"] {
        for (rate, channels) in [(48_000, 2), (44_100, 2), (96_000, 1)] {
            let dir = tempfile::tempdir().unwrap();
            let lengths = [1, 7, 255, 1, 4709, 24_000, 1, 5, 33_021];
            let pcm = samples(lengths.iter().sum(), channels);
            let write = if extension == "wav" {
                write_wav
            } else {
                write_flac
            };
            let full = dir.path().join(format!("full.{extension}"));
            write(&full, rate, channels, &pcm);
            let mut offset = 0;
            let paths: Vec<_> = lengths
                .iter()
                .enumerate()
                .map(|(index, length)| {
                    let path = dir.path().join(format!("{index}.{extension}"));
                    let end = offset + length * usize::from(channels);
                    write(&path, rate, channels, &pcm[offset..end]);
                    offset = end;
                    path
                })
                .collect();
            let (unsplit, _, _) = render_sequence(&[full], 48_000);
            let (split, harness, ids) = render_sequence(&paths, 48_000);
            assert_eq!(split.len(), unsplit.len(), "{extension} {rate} {channels}");
            assert!(
                split == unsplit,
                "PCM differs: {extension} {rate} {channels}"
            );
            assert!(harness.format_changes.is_empty());
            assert_eq!(harness.boundaries.len(), ids.len() + 1);
            let mut source_frame = 0u64;
            for (index, id) in ids.iter().enumerate() {
                let boundary = harness.boundaries[index];
                assert_eq!(boundary.1, index.checked_sub(1).map(|index| ids[index]));
                assert_eq!(boundary.2, Some(*id));
                assert_eq!(
                    boundary.3,
                    (source_frame * 48_000 + u64::from(rate) / 2) / u64::from(rate)
                );
                source_frame += lengths[index] as u64;
            }
            assert_eq!(harness.boundaries.last().unwrap().2, None);
            if rate == 48_000 {
                let expected: Vec<_> = pcm
                    .iter()
                    .map(|sample| f32::from(*sample) / 32768.0)
                    .collect();
                assert!(split == expected);
            }
        }
    }
}

#[test]
fn one_frame_entries_advance_without_controller_or_ui_commands_and_keep_every_boundary() {
    let dir = tempfile::tempdir().unwrap();
    let paths: Vec<_> = (0..300)
        .map(|index| {
            let path = dir.path().join(format!("{index}.wav"));
            write_wav(&path, 48_000, 2, &[1000 + index, -1000 - index]);
            path
        })
        .collect();
    let (rendered, harness, ids) = render_sequence(&paths, 48_000);
    let expected: Vec<_> = (0..300)
        .flat_map(|index| {
            [
                (1000 + index) as f32 / 32768.0,
                (-1000 - index) as f32 / 32768.0,
            ]
        })
        .collect();
    assert!(rendered == expected);
    assert_eq!(harness.boundaries.len(), 301);
    for (index, id) in ids.iter().enumerate() {
        assert_eq!(harness.boundaries[index].2, Some(*id));
        assert_eq!(harness.boundaries[index].3, index as u64);
    }
}

#[test]
fn source_rate_and_layout_changes_are_explicit_and_keep_the_device_stream() {
    let dir = tempfile::tempdir().unwrap();
    let first = dir.path().join("stereo.wav");
    let second = dir.path().join("mono.wav");
    let third = dir.path().join("resampled.wav");
    write_wav(&first, 48_000, 2, &samples(3001, 2));
    write_wav(&second, 48_000, 1, &samples(3011, 1));
    write_wav(&third, 44_100, 1, &samples(3023, 1));
    let (actual, harness, ids) =
        render_sequence(&[first.clone(), second.clone(), third.clone()], 48_000);
    let mut expected = Vec::new();
    for path in [first, second, third] {
        expected.extend(render_sequence(&[path], 48_000).0);
    }
    assert!(actual == expected);
    assert_eq!(harness.format_changes, ids[1..]);
}

#[test]
fn lossy_delay_and_padding_are_removed_once_at_the_backend_boundary() {
    for name in ["lossy/sine.mp3", "lossy/sine.m4a"] {
        let path = fixture(name);
        let mut expected = decoded(&path);
        expected.extend_from_within(..);
        let (actual, harness, _) = render_sequence(&[path.clone(), path], 44_100);
        assert_eq!(actual.len(), 88_200 * 2, "{name}");
        assert!(actual == expected, "{name}");
        assert!(
            harness
                .latest
                .unwrap()
                .source
                .unwrap()
                .encoder_delay
                .trimmed
        );
    }
}

#[test]
fn selected_entry_restores_at_saved_position_paused_then_continues_the_sequence() {
    let dir = tempfile::tempdir().unwrap();
    let paths: Vec<_> = (0..3)
        .map(|index| {
            let path = dir.path().join(format!("{index}.wav"));
            write_wav(&path, 48_000, 2, &samples(5000 + index, 2));
            path
        })
        .collect();
    let mut harness = Harness::new(48_000);
    let (sequence, ids) = sequence(&paths, RepeatMode::Off);
    harness
        .engine
        .start_sequence(sequence, ids[1], Duration::from_millis(50), true)
        .unwrap();
    let restored = harness.wait("restored and buffered", |snapshot| {
        snapshot.playback.current == Some(ids[1])
            && snapshot
                .playback
                .position
                .is_some_and(|position| position.frame == 2400)
            && snapshot.playback.transport == TransportState::Paused
    });
    harness.drain_for(Duration::from_millis(50));
    assert_eq!(harness.capture.total_frames(), 0);
    harness.engine.resume();
    let ended = harness.wait("restored sequence ended", |snapshot| {
        snapshot.playback.current == Some(ids[2])
            && snapshot.playback.transport == TransportState::Stopped
    });
    assert_eq!(ended.playback.generation, restored.playback.generation);
    let actual = harness
        .capture
        .rendered_epoch(ended.playback.generation.get());
    let mut expected = decoded(&paths[1])[2400 * 2..].to_vec();
    expected.extend(decoded(&paths[2]));
    assert!(actual == expected);
}

#[test]
fn repeat_one_and_all_use_the_supplied_order_and_continuous_converter_history() {
    for (repeat, rate, lengths) in [
        (RepeatMode::One, 48_000, [1, 3, 7]),
        (RepeatMode::All, 48_000, [1, 3, 7]),
        (RepeatMode::One, 44_100, [13, 17, 19]),
        (RepeatMode::All, 44_100, [13, 17, 19]),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let paths: Vec<_> = lengths
            .iter()
            .enumerate()
            .map(|(index, frames)| {
                let path = dir.path().join(format!("{index}.wav"));
                let pcm = vec![1000 + index as i16 * 1000; frames * 2];
                write_wav(&path, rate, 2, &pcm);
                path
            })
            .collect();
        let order = [paths[2].clone(), paths[0].clone(), paths[1].clone()];
        let (sequence, ids) = sequence(&order, repeat);
        let mut harness = Harness::new(48_000);
        harness
            .engine
            .start_sequence(sequence.clone(), ids[1], Duration::ZERO, false)
            .unwrap();
        let playing = harness.wait_transport(TransportState::Playing);
        let generation = playing.playback.generation.get();
        let deadline = Instant::now() + Duration::from_secs(5);
        while harness.capture.epoch(generation).len() < 8192 * 2 {
            assert!(Instant::now() < deadline);
            harness.drain_for(Duration::from_millis(5));
        }
        let actual = harness.capture.rendered_epoch(generation);
        let mut input = Vec::new();
        let mut index = 1;
        while input.len() < 20_000 * 2 {
            input.extend(decoded(&order[index]).into_iter().map(f64::from));
            index = sequence.next(index).unwrap();
        }
        let expected: Vec<f32> = if rate == 48_000 {
            input.into_iter().map(|value| value as f32).collect()
        } else {
            let mut converter =
                super::super::resample::RateConverter::new(rate, 48_000, 2).unwrap();
            let mut output = Vec::new();
            converter.process(&input, &mut output);
            converter.finish(&mut output);
            output.into_iter().map(|value| value as f32).collect()
        };
        assert!(
            actual[..8192 * 2] == expected[..8192 * 2],
            "{repeat:?} {rate}"
        );
        let observed: Vec<_> = harness
            .boundaries
            .iter()
            .filter_map(|boundary| boundary.2)
            .take(20)
            .collect();
        let expected_ids: Vec<_> = if repeat == RepeatMode::One {
            vec![ids[1]; 20]
        } else {
            (0..20).map(|index| ids[(index + 1) % 3]).collect()
        };
        assert_eq!(observed, expected_ids);
        assert_eq!(*harness.capture.opens.lock().unwrap(), 1);
        harness.engine.stop();
        harness.wait("stopped repeat", |snapshot| {
            snapshot.playback.current.is_none()
        });
    }
}

#[test]
fn seek_and_queue_edit_cancel_prefetched_entries_and_keep_the_requested_position() {
    let dir = tempfile::tempdir().unwrap();
    let paths: Vec<_> = (0..3)
        .map(|index| {
            let path = dir.path().join(format!("{index}.wav"));
            write_wav(&path, 48_000, 2, &samples(5000 + index, 2));
            path
        })
        .collect();
    let (sequence, ids) = sequence(&paths, RepeatMode::Off);
    let mut harness = Harness::new(48_000);
    harness
        .engine
        .start_sequence(sequence.clone(), ids[0], Duration::ZERO, true)
        .unwrap();
    let loaded = harness.wait("initial entry loaded", |snapshot| snapshot.source.is_some());
    harness.drain_for(Duration::from_millis(50));
    harness.engine.seek(ids[0], Duration::from_millis(10));
    let replacement = PlaybackSequence::new(
        vec![sequence.entries[0].clone(), sequence.entries[2].clone()],
        RepeatMode::Off,
    )
    .unwrap();
    harness.engine.update_sequence(replacement);
    let edited = harness.wait("edited and sought", |snapshot| {
        snapshot.playback.generation.get() >= loaded.playback.generation.get() + 2
            && snapshot
                .playback
                .position
                .is_some_and(|position| position.frame == 480)
    });
    harness.engine.seek_in_generation(
        loaded.playback.generation,
        ids[0],
        Duration::from_millis(30),
    );
    harness.engine.resume();
    let ended = harness.wait("edited sequence ended", |snapshot| {
        snapshot.playback.current == Some(ids[2])
            && snapshot.playback.transport == TransportState::Stopped
    });
    assert_eq!(ended.playback.generation, edited.playback.generation);
    let actual = harness
        .capture
        .rendered_epoch(ended.playback.generation.get());
    let mut expected = decoded(&paths[0])[480 * 2..].to_vec();
    expected.extend(decoded(&paths[2]));
    assert!(actual == expected);
    assert!(
        !harness
            .boundaries
            .iter()
            .any(|boundary| boundary.2 == Some(ids[1]))
    );
}

#[test]
fn a_missing_prefetched_entry_waits_for_current_audio_and_reports_the_failed_identity() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("valid.wav");
    write_wav(&path, 44_100, 2, &samples(5003, 2));
    let missing = dir.path().join("missing.wav");
    let (sequence, ids) = sequence(&[path.clone(), missing], RepeatMode::All);
    let mut harness = Harness::new(48_000);
    harness
        .engine
        .start_sequence(sequence, ids[0], Duration::ZERO, false)
        .unwrap();
    let failed = harness.wait("upcoming entry failed", |snapshot| {
        snapshot.failure.is_some()
    });
    assert_eq!(failed.failure.as_ref().unwrap().entry, Some(ids[1]));
    let actual = harness
        .capture
        .rendered_epoch(failed.playback.generation.get());
    assert!(actual == render_sequence(&[path], 48_000).0);
    let previous_boundary_count = harness.boundaries.len();
    harness.drain_for(Duration::from_millis(50));
    assert_eq!(harness.boundaries.len(), previous_boundary_count);
    assert_eq!(
        harness.latest.as_ref().unwrap().playback.transport,
        TransportState::Stopped
    );
}

#[test]
fn empty_repeated_files_fail_once_instead_of_spinning() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("empty.wav");
    write_wav(&path, 48_000, 2, &[]);
    let (sequence, ids) = sequence(&[path], RepeatMode::One);
    let mut harness = Harness::new(48_000);
    harness
        .engine
        .start_sequence(sequence, ids[0], Duration::ZERO, false)
        .unwrap();
    let failed = harness.wait("empty entry failure", |snapshot| snapshot.failure.is_some());
    assert_eq!(failed.failure.unwrap().entry, Some(ids[0]));
    harness.drain_for(Duration::from_millis(50));
    assert_eq!(
        harness.latest.as_ref().unwrap().playback.transport,
        TransportState::Stopped
    );
    assert!(harness.boundaries.len() <= 2);
}

#[test]
fn removing_the_current_entry_finishes_it_once_and_excludes_it_from_repeat() {
    let dir = tempfile::tempdir().unwrap();
    let paths: Vec<_> = (0..2)
        .map(|index| {
            let path = dir.path().join(format!("{index}.wav"));
            write_wav(&path, 48_000, 2, &vec![1000 + index * 1000; 5000 * 2]);
            path
        })
        .collect();
    let (original, ids) = sequence(&paths, RepeatMode::All);
    let mut harness = Harness::new(48_000);
    harness
        .engine
        .start_sequence(original.clone(), ids[0], Duration::ZERO, true)
        .unwrap();
    let initial = harness.wait("loaded current", |snapshot| snapshot.source.is_some());
    harness.engine.update_sequence(
        PlaybackSequence::new(vec![original.entries[1].clone()], RepeatMode::All).unwrap(),
    );
    let edited = harness.wait("current removed", |snapshot| {
        snapshot.playback.generation > initial.playback.generation && snapshot.source.is_some()
    });
    harness.engine.resume();
    harness.wait_transport(TransportState::Playing);
    let generation = edited.playback.generation.get();
    let deadline = Instant::now() + Duration::from_secs(5);
    while harness.capture.epoch(generation).len() < 16_000 * 2 {
        assert!(Instant::now() < deadline);
        harness.drain_for(Duration::from_millis(5));
    }
    let actual = harness.capture.rendered_epoch(generation);
    let mut expected = decoded(&paths[0]);
    for _ in 0..3 {
        expected.extend(decoded(&paths[1]));
    }
    assert!(actual[..16_000 * 2] == expected[..16_000 * 2]);
    assert!(
        !harness
            .boundaries
            .iter()
            .any(|boundary| boundary.0 == generation && boundary.2 == Some(ids[0]))
    );
    harness.engine.stop();
    harness.wait("repeat stopped", |snapshot| {
        snapshot.playback.current.is_none()
    });
}

#[test]
fn an_empty_entry_after_a_resampled_track_does_not_drop_the_converter_tail() {
    let dir = tempfile::tempdir().unwrap();
    let first = dir.path().join("first.wav");
    let empty = dir.path().join("empty.wav");
    write_wav(&first, 44_100, 2, &samples(333, 2));
    write_wav(&empty, 44_100, 2, &[]);
    let (sequence, ids) = sequence(&[first.clone(), empty], RepeatMode::Off);
    let mut harness = Harness::new(48_000);
    harness
        .engine
        .start_sequence(sequence, ids[0], Duration::ZERO, false)
        .unwrap();
    let failed = harness.wait("empty entry failed", |snapshot| snapshot.failure.is_some());
    assert_eq!(failed.failure.unwrap().entry, Some(ids[1]));
    assert!(
        harness
            .capture
            .rendered_epoch(failed.playback.generation.get())
            == render_sequence(&[first], 48_000).0
    );
}

fn write_extensible_wav(path: &Path, mask: u32, pcm: &[i16]) {
    write_wav(path, 44_100, 2, pcm);
    let mut wav = std::fs::read(path).unwrap();
    let size = wav.len() as u32 - 8 + 24;
    wav[4..8].copy_from_slice(&size.to_le_bytes());
    wav[16..20].copy_from_slice(&40u32.to_le_bytes());
    wav[20..22].copy_from_slice(&0xfffeu16.to_le_bytes());
    let mut extension = Vec::new();
    extension.extend_from_slice(&22u16.to_le_bytes());
    extension.extend_from_slice(&16u16.to_le_bytes());
    extension.extend_from_slice(&mask.to_le_bytes());
    extension.extend_from_slice(&[
        1, 0, 0, 0, 0, 0, 0x10, 0, 0x80, 0, 0, 0xaa, 0, 0x38, 0x9b, 0x71,
    ]);
    wav.splice(36..36, extension);
    std::fs::write(path, wav).unwrap();
}

#[test]
fn equal_channel_counts_with_different_speaker_layouts_reconfigure_explicitly() {
    let dir = tempfile::tempdir().unwrap();
    let first = dir.path().join("front.wav");
    let second = dir.path().join("rear.wav");
    write_extensible_wav(&first, 0x03, &samples(3001, 2));
    write_extensible_wav(&second, 0x30, &samples(3001, 2));
    assert!(
        !Decoder::open(&first)
            .unwrap()
            .info()
            .continuous_with(Decoder::open(&second).unwrap().info())
    );
    let (actual, harness, ids) = render_sequence(&[first.clone(), second.clone()], 48_000);
    assert_eq!(harness.format_changes, ids[1..]);
    let mut expected = render_sequence(&[first], 48_000).0;
    expected.extend(render_sequence(&[second], 48_000).0);
    assert!(actual == expected);
}

#[test]
fn paused_restoration_holds_the_selected_entry_when_a_boundary_rounds_to_zero_output_frames() {
    let dir = tempfile::tempdir().unwrap();
    let first = dir.path().join("one.wav");
    let second = dir.path().join("rest.wav");
    write_wav(&first, 192_000, 2, &[1234, -1234]);
    write_wav(&second, 192_000, 2, &samples(5000, 2));
    let (sequence, ids) = sequence(&[first, second], RepeatMode::Off);
    let mut harness = Harness::new(48_000);
    harness
        .engine
        .start_sequence(sequence, ids[0], Duration::ZERO, true)
        .unwrap();
    harness.wait("selected tiny entry loaded", |snapshot| {
        snapshot.source.is_some()
    });
    harness.drain_for(Duration::from_millis(50));
    assert_eq!(
        harness.latest.as_ref().unwrap().playback.current,
        Some(ids[0])
    );
    assert_eq!(harness.capture.total_frames(), 0);
    harness.engine.resume();
    let ended = harness.wait("short downsampled sequence ended", |snapshot| {
        snapshot.playback.current == Some(ids[1])
            && snapshot.playback.transport == TransportState::Stopped
    });
    assert!(ended.failure.is_none());
    assert_eq!(
        harness
            .capture
            .rendered_epoch(ended.playback.generation.get())
            .len(),
        1250 * 2
    );
    assert_eq!(harness.boundaries.len(), 3);
    assert_eq!(harness.boundaries[1].3, 0);
}

#[test]
fn ui_coalescing_keeps_all_tiny_entry_boundaries_in_order() {
    let generation = parabolica_domain::control::GenerationCounter::default().advance();
    let ids: Vec<_> = (0..300).map(|_| QueueEntryId::generate()).collect();
    let events = ids
        .iter()
        .enumerate()
        .flat_map(|(index, entry)| {
            [
                EngineEvent::EntryBoundary {
                    generation,
                    from: index.checked_sub(1).map(|index| ids[index]),
                    to: Some(*entry),
                    output_frame: index as u64,
                },
                EngineEvent::Underruns {
                    total: index as u64,
                },
            ]
        })
        .collect();
    let coalesced = parabolica_domain::dispatch::coalesce(events);
    let entries: Vec<_> = coalesced
        .iter()
        .filter_map(|event| match event {
            EngineEvent::EntryBoundary { to, .. } => *to,
            _ => None,
        })
        .collect();
    assert_eq!(entries, ids);
    assert_eq!(coalesced.len(), 301);
}

#[test]
fn output_loss_after_an_entry_boundary_preserves_the_entry_relative_resume_position() {
    let dir = tempfile::tempdir().unwrap();
    let first = dir.path().join("first.wav");
    write_wav(&first, 48_000, 2, &samples(5000, 2));
    let second = counting_wav(dir.path(), "long.wav", 8, 2000);
    let (sequence, ids) = sequence(&[first, second], RepeatMode::Off);
    let mut harness = Harness::new(48_000);
    harness
        .engine
        .start_sequence(sequence, ids[0], Duration::ZERO, false)
        .unwrap();
    harness.wait("second entry playing", |snapshot| {
        snapshot.playback.current == Some(ids[1])
            && snapshot.playback.transport == TransportState::Playing
    });
    *harness.capture.lost.lock().unwrap() = Some(OutputError::DeviceLost);
    let blocked = harness.wait_transport(TransportState::Blocked);
    assert_eq!(blocked.playback.current, Some(ids[1]));
    let frame = blocked.playback.position.unwrap().frame;
    assert!(frame < 48_000);
    harness.engine.resume();
    let resumed = harness.wait_transport(TransportState::Playing);
    let generation = resumed.playback.generation.get();
    let deadline = Instant::now() + Duration::from_secs(5);
    while harness.capture.epoch(generation).len() < 256 * 2 {
        assert!(Instant::now() < deadline);
        harness.drain_for(Duration::from_millis(5));
    }
    let captured = harness.capture.epoch(generation);
    assert_eq!(counting_frame(captured[0]), frame % 32_768);
    assert_eq!(captured[1], 2000.0 / 32_768.0);
    assert_eq!(*harness.capture.opens.lock().unwrap(), 2);
}
