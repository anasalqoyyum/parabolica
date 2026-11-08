//! Controller tests through a capture backend that records what the device would receive.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use parabolica_domain::format::{ChannelCount, OutputFormat, OutputSampleFormat, SampleRate};
use parabolica_domain::gain::Volume;
use parabolica_domain::ids::{MediaFileId, QueueEntryId};
use parabolica_domain::output::{
    BufferConfig, BufferOwner, NegotiatedOutput, OutputBackend, OutputDeviceId,
};
use parabolica_domain::playback::{PositionStamp, TransportState};
use parabolica_domain::signal_path::{StageKind, StageStatus};
use parabolica_domain::source::ResolvedSource;

use super::*;
use crate::decode::Decoder;
use crate::realtime::{RealtimeRender, RenderOutcome};
use crate::transitions::PlaybackSequence;
use parabolica_domain::queue::RepeatMode;

#[path = "output_tests.rs"]
mod output_tests;
#[path = "sequence_tests.rs"]
mod sequence_tests;

const BLOCK_FRAMES: usize = 256;

/// Frames the renderer consumed, grouped by the epoch (generation) they were played in.
#[derive(Debug, Default)]
struct Captured {
    blocks: Vec<(u64, Vec<f32>)>,
    callbacks: Vec<(u64, Vec<f32>, usize, RenderOutcome)>,
}

impl Captured {
    fn epoch(&self, epoch: u64) -> Vec<f32> {
        self.blocks
            .iter()
            .filter(|(block_epoch, _)| *block_epoch == epoch)
            .flat_map(|(_, samples)| samples.iter().copied())
            .collect()
    }

    fn total_frames(&self) -> usize {
        self.blocks
            .iter()
            .map(|(_, samples)| samples.len())
            .sum::<usize>()
            / 2
    }
}

#[derive(Clone)]
struct Capture {
    rate: u32,
    open_error: Arc<Mutex<Option<OutputError>>>,
    lost: Arc<Mutex<Option<OutputError>>>,
    captured: Arc<Mutex<Captured>>,
    opens: Arc<Mutex<usize>>,
}

impl Capture {
    fn new(rate: u32) -> Self {
        Self {
            rate,
            open_error: Arc::default(),
            lost: Arc::default(),
            captured: Arc::default(),
            opens: Arc::default(),
        }
    }

    fn epoch(&self, epoch: u64) -> Vec<f32> {
        self.captured.lock().unwrap().epoch(epoch)
    }

    fn total_frames(&self) -> usize {
        self.captured.lock().unwrap().total_frames()
    }
}

impl Output for Capture {
    fn open(&mut self) -> Result<Box<dyn OutputStream>, OutputError> {
        if let Some(error) = self.open_error.lock().unwrap().clone() {
            return Err(error);
        }
        *self.opens.lock().unwrap() += 1;
        *self.lost.lock().unwrap() = None;
        Ok(Box::new(CaptureStream {
            negotiated: NegotiatedOutput {
                device: OutputDeviceId {
                    backend: OutputBackend::WasapiShared,
                    stable_id: "capture".to_owned(),
                },
                device_name: "Capture".to_owned(),
                format: OutputFormat {
                    sample: OutputSampleFormat::Float { bits: 32 },
                    rate: SampleRate::new(self.rate).unwrap(),
                    channels: ChannelCount::STEREO,
                },
                channel_map: vec![0, 1],
                buffer: BufferConfig {
                    frames: BLOCK_FRAMES as u32,
                    owner: BufferOwner::App,
                },
                reported_latency: None,
            },
            capture: self.clone(),
            stop: Arc::default(),
            thread: None,
        }))
    }
}

struct CaptureStream {
    negotiated: NegotiatedOutput,
    capture: Capture,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl OutputStream for CaptureStream {
    fn negotiated(&self) -> &NegotiatedOutput {
        &self.negotiated
    }

    fn start(&mut self, mut renderer: StreamRenderer) -> Result<(), OutputError> {
        let capture = self.capture.clone();
        let stop = self.stop.clone();
        self.thread = Some(std::thread::spawn(move || {
            let shared = renderer.shared().clone();
            let mut block = vec![0.0f32; BLOCK_FRAMES * 2];
            // About five times faster than real time at 48 kHz.
            while !stop.load(Ordering::Acquire) && capture.lost.lock().unwrap().is_none() {
                let epoch_before = shared.epoch_ack.load(Ordering::Acquire);
                let played_before = shared.played.load(Ordering::Acquire);
                let playing = shared.playing.load(Ordering::Acquire);
                let outcome = renderer.render(&mut block[..], 2);
                let epoch = shared.epoch_ack.load(Ordering::Acquire);
                let played = shared.played.load(Ordering::Acquire);
                let consumed = if epoch == epoch_before {
                    played - played_before
                } else {
                    played
                } as usize;
                if playing {
                    capture.captured.lock().unwrap().callbacks.push((
                        epoch,
                        block.clone(),
                        consumed,
                        outcome,
                    ));
                }
                if consumed > 0 {
                    capture
                        .captured
                        .lock()
                        .unwrap()
                        .blocks
                        .push((epoch, block[..consumed * 2].to_vec()));
                }
                std::thread::sleep(Duration::from_millis(1));
            }
        }));
        Ok(())
    }

    fn failure(&self) -> Option<OutputError> {
        self.capture.lost.lock().unwrap().clone()
    }
}

impl Drop for CaptureStream {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            thread.join().unwrap();
        }
    }
}

struct Harness {
    engine: PlaybackEngine,
    events: Receiver<EngineEvent>,
    capture: Capture,
    latest: Option<EngineSnapshot>,
    position: Option<PositionStamp>,
    lost: Vec<OutputDeviceId>,
    boundaries: Vec<(u64, Option<QueueEntryId>, Option<QueueEntryId>, u64)>,
    format_changes: Vec<QueueEntryId>,
}

impl Harness {
    fn new(rate: u32) -> Self {
        let capture = Capture::new(rate);
        let (engine, events) = PlaybackEngine::start(Box::new(capture.clone()));
        Self {
            engine,
            events,
            capture,
            latest: None,
            position: None,
            lost: Vec::new(),
            boundaries: Vec::new(),
            format_changes: Vec::new(),
        }
    }

    fn play(&self, path: &Path) -> QueueEntryId {
        let entry = QueueEntryId::generate();
        self.engine.play(
            entry,
            ResolvedSource::local_file(MediaFileId::generate(), path.to_owned()),
        );
        entry
    }

    /// Wait for a snapshot that satisfies `condition` and return it.
    fn wait(&mut self, what: &str, condition: impl Fn(&EngineSnapshot) -> bool) -> EngineSnapshot {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(latest) = &self.latest
                && condition(latest)
            {
                return latest.clone();
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            match self.events.recv_timeout(remaining) {
                Ok(event) => self.apply(event),
                Err(_) => panic!("timed out waiting for {what}; last {:#?}", self.latest),
            }
        }
    }

    fn apply(&mut self, event: EngineEvent) {
        match event {
            EngineEvent::Snapshot(snapshot) => {
                self.position = snapshot.state.playback.position;
                self.latest = Some(snapshot.state);
            }
            EngineEvent::Position(position) => self.position = Some(position),
            EngineEvent::OutputLost { device } => self.lost.push(device),
            EngineEvent::Underruns { .. } | EngineEvent::Synchronized { .. } => {}
            EngineEvent::EntryBoundary {
                generation,
                from,
                to,
                output_frame,
            } => self
                .boundaries
                .push((generation.get(), from, to, output_frame)),
            EngineEvent::SourceFormatChanged { entry, .. } => self.format_changes.push(entry),
        }
    }

    /// Apply every event that arrives within `period`.
    fn drain_for(&mut self, period: Duration) {
        let deadline = Instant::now() + period;
        while let Ok(event) = self
            .events
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
        {
            self.apply(event);
        }
    }

    fn wait_transport(&mut self, transport: TransportState) -> EngineSnapshot {
        self.wait(&format!("{transport:?}"), |snapshot| {
            snapshot.playback.transport == transport
        })
    }
}

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn decoded(path: &Path) -> Vec<f32> {
    let mut decoder = Decoder::open(path).unwrap();
    let mut samples = Vec::new();
    while decoder.next_block(&mut samples).unwrap() > 0 {}
    samples.into_iter().map(|sample| sample as f32).collect()
}

/// Write a 16-bit stereo 48 kHz WAV whose left channel counts frames, so any captured frame
/// identifies its source position.
fn counting_wav(dir: &Path, name: &str, seconds: u32, offset: i16) -> PathBuf {
    let frames = 48_000 * seconds;
    let mut data = Vec::with_capacity(frames as usize * 4);
    for frame in 0..frames {
        let left = (frame % 32_768) as i16;
        data.extend_from_slice(&left.to_le_bytes());
        data.extend_from_slice(&offset.to_le_bytes());
    }
    let mut wav = Vec::new();
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&2u16.to_le_bytes());
    wav.extend_from_slice(&48_000u32.to_le_bytes());
    wav.extend_from_slice(&(48_000u32 * 4).to_le_bytes());
    wav.extend_from_slice(&4u16.to_le_bytes());
    wav.extend_from_slice(&16u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&(data.len() as u32).to_le_bytes());
    wav.extend_from_slice(&data);
    let path = dir.join(name);
    std::fs::write(&path, wav).unwrap();
    path
}

fn counting_frame(sample: f32) -> u64 {
    (f64::from(sample) * 32_768.0).round() as u64
}

#[test]
fn plays_flac_bit_exactly_at_the_device_rate_and_reports_the_route() {
    let mut harness = Harness::new(48_000);
    let path = fixture("flac/s24_48000.flac");
    let entry = harness.play(&path);

    let playing = harness.wait_transport(TransportState::Playing);
    assert_eq!(playing.playback.current, Some(entry));
    assert_eq!(playing.playback.duration, Some(Duration::from_millis(200)));
    let route = playing.playback.route.as_ref().unwrap();
    assert!(!route.requires_resampling());
    let path_stages = playing.playback.signal_path.as_ref().unwrap();
    for kind in [
        StageKind::Processing,
        StageKind::Resampling,
        StageKind::Conversion,
    ] {
        assert_eq!(
            path_stages.stage(kind).unwrap().status,
            StageStatus::Bypassed,
            "{kind:?}"
        );
    }
    assert!(!path_stages.app_changes_samples());

    let ended = harness.wait_transport(TransportState::Stopped);
    assert_eq!(ended.playback.current, Some(entry));
    assert!(ended.failure.is_none());
    assert_eq!(ended.underruns, 0);
    let generation = ended.playback.generation.get();
    assert!(harness.capture.epoch(generation) == decoded(&path));
    let position = ended.playback.position.unwrap();
    assert_eq!(position.frame, 9600);
    assert!(!position.advancing);
}

#[test]
fn converts_the_rate_in_one_stage_when_the_device_differs() {
    let mut harness = Harness::new(48_000);
    harness.play(&fixture("flac/s24_96000.flac"));
    let playing = harness.wait_transport(TransportState::Playing);
    let stages = playing.playback.signal_path.as_ref().unwrap();
    assert_eq!(
        stages.stage(StageKind::Resampling).unwrap().status,
        StageStatus::Active
    );
    assert!(
        playing
            .playback
            .route
            .as_ref()
            .unwrap()
            .requires_resampling()
    );

    let ended = harness.wait_transport(TransportState::Stopped);
    let captured = harness.capture.epoch(ended.playback.generation.get());
    assert_eq!(captured.len(), 9600 * 2);
    // The left channel is a 997 Hz sine at 0.9 of full scale; check away from the edges and the
    // full-scale steps at frames 100 and 101 of the source.
    let full = f64::from((1 << 23) - 1) / f64::from(1 << 23);
    let error = (2000..8000)
        .map(|frame| {
            let expected =
                0.9 * full * (2.0 * std::f64::consts::PI * 997.0 * frame as f64 / 48_000.0).sin();
            (f64::from(captured[frame * 2]) - expected).abs()
        })
        .fold(0.0, f64::max);
    assert!(error < 1e-3, "max error {error}");
}

#[test]
fn pause_holds_a_truthful_position_and_resume_continues() {
    let dir = tempfile::tempdir().unwrap();
    let path = counting_wav(dir.path(), "long.wav", 8, 0);
    let mut harness = Harness::new(48_000);
    harness.play(&path);
    let playing = harness.wait_transport(TransportState::Playing);
    let generation = playing.playback.generation.get();
    while harness.capture.total_frames() < 48_000 {
        std::thread::sleep(Duration::from_millis(5));
    }

    harness.engine.pause();
    harness.wait_transport(TransportState::Paused);
    // Let the fade-out finish and the corrected position arrive.
    harness.drain_for(Duration::from_millis(150));
    let frames = harness.capture.epoch(generation).len() / 2;
    let position = harness.position.unwrap();
    assert!(!position.advancing);
    assert_eq!(position.frame, frames as u64);
    harness.drain_for(Duration::from_millis(100));
    assert_eq!(harness.capture.epoch(generation).len() / 2, frames);

    harness.engine.resume();
    harness.wait_transport(TransportState::Playing);
    while harness.capture.epoch(generation).len() / 2 < frames + 4800 {
        std::thread::sleep(Duration::from_millis(5));
    }
    // After the fade-in, frames continue exactly where the pause left them.
    let captured = harness.capture.epoch(generation);
    let resumed: Vec<u64> = captured[(frames + 1000) * 2..(frames + 4000) * 2]
        .iter()
        .step_by(2)
        .map(|sample| counting_frame(*sample))
        .collect();
    let expected: Vec<u64> = ((frames + 1000) as u64..(frames + 4000) as u64)
        .map(|frame| frame % 32_768)
        .collect();
    assert_eq!(resumed, expected);
    assert_eq!(harness.latest.as_ref().unwrap().underruns, 0);
}

#[test]
fn seek_starts_at_the_exact_frame_without_old_audio() {
    let dir = tempfile::tempdir().unwrap();
    let path = counting_wav(dir.path(), "long.wav", 8, 0);
    let mut harness = Harness::new(48_000);
    let entry = harness.play(&path);
    harness.wait_transport(TransportState::Playing);

    harness.engine.seek(entry, Duration::from_millis(2_500));
    let seeked = harness.wait("seeked and playing", |snapshot| {
        snapshot.playback.transport == TransportState::Playing
            && snapshot
                .playback
                .position
                .is_some_and(|position| position.frame >= 120_000)
    });
    while harness
        .capture
        .epoch(seeked.playback.generation.get())
        .len()
        < 2 * 4800
    {
        std::thread::sleep(Duration::from_millis(5));
    }
    let captured = harness.capture.epoch(seeked.playback.generation.get());
    let frames: Vec<u64> = captured
        .iter()
        .step_by(2)
        .map(|s| counting_frame(*s))
        .collect();
    let expected: Vec<u64> = (120_000..120_000 + frames.len() as u64)
        .map(|frame| frame % 32_768)
        .collect();
    assert_eq!(frames, expected);
}

#[test]
fn stale_seeks_cannot_affect_a_newly_selected_file() {
    let dir = tempfile::tempdir().unwrap();
    let first = counting_wav(dir.path(), "first.wav", 8, 1000);
    let second = fixture("flac/s24_48000.flac");
    let mut harness = Harness::new(48_000);
    let first_entry = harness.play(&first);
    harness.wait_transport(TransportState::Playing);

    // A seek on the first file, a new selection, then a late seek still aimed at the first file.
    harness.engine.seek(first_entry, Duration::from_secs(5));
    let second_entry = harness.play(&second);
    harness.engine.seek(first_entry, Duration::from_secs(6));

    let ended = harness.wait("second file ended", |snapshot| {
        snapshot.playback.current == Some(second_entry)
            && snapshot.playback.transport == TransportState::Stopped
    });
    assert!(ended.failure.is_none());
    let captured = harness.capture.epoch(ended.playback.generation.get());
    assert!(
        captured == decoded(&second),
        "the second file played exactly from its start"
    );
    assert_eq!(ended.playback.position.unwrap().frame, 9600);
}

#[test]
fn volume_scales_samples_and_marks_processing_active() {
    let mut harness = Harness::new(44_100);
    harness.engine.set_volume(Volume::new(0.5).unwrap());
    let path = fixture("flac/s16_44100.flac");
    harness.play(&path);
    let playing = harness.wait_transport(TransportState::Playing);
    assert_eq!(playing.playback.volume, Volume::new(0.5).unwrap());
    let processing = playing
        .playback
        .signal_path
        .as_ref()
        .unwrap()
        .stage(StageKind::Processing)
        .unwrap()
        .status;
    assert_eq!(processing, StageStatus::Active);
    let ended = harness.wait_transport(TransportState::Stopped);
    let captured = harness.capture.epoch(ended.playback.generation.get());
    let expected: Vec<f32> = decoded(&path).iter().map(|sample| sample * 0.5).collect();
    assert!(captured == expected);
}

#[test]
fn ended_entry_replays_from_the_start_on_resume() {
    let mut harness = Harness::new(48_000);
    let path = fixture("flac/s24_48000.flac");
    harness.play(&path);
    let first = harness.wait_transport(TransportState::Stopped);
    harness.engine.resume();
    let replay = harness.wait("replayed", |snapshot| {
        snapshot.playback.transport == TransportState::Stopped
            && snapshot.playback.generation > first.playback.generation
    });
    assert!(harness.capture.epoch(replay.playback.generation.get()) == decoded(&path));
}

#[test]
fn bad_files_fail_visibly_and_the_engine_keeps_working() {
    let dir = tempfile::tempdir().unwrap();
    let bytes = std::fs::read(fixture("flac/s24_96000.flac")).unwrap();
    let mut corrupt = bytes.clone();
    let middle = corrupt.len() / 2;
    for byte in &mut corrupt[middle..middle + 64] {
        *byte ^= 0x5a;
    }
    let corrupt_path = dir.path().join("corrupt.flac");
    std::fs::write(&corrupt_path, &corrupt).unwrap();

    let mut harness = Harness::new(48_000);
    let entry = harness.play(&corrupt_path);
    let failed = harness.wait("corrupt failure", |snapshot| snapshot.failure.is_some());
    let failure = failed.failure.unwrap();
    assert_eq!(failure.entry, Some(entry));
    assert!(
        failure
            .error
            .to_string()
            .starts_with("the file is corrupt at"),
        "{}",
        failure.error
    );
    assert_eq!(failed.playback.transport, TransportState::Stopped);
    assert_eq!(std::fs::read(&corrupt_path).unwrap(), corrupt);

    harness.play(&fixture("unsupported/opus.ogg"));
    let unsupported = harness.wait("unsupported failure", |snapshot| {
        snapshot
            .failure
            .as_ref()
            .is_some_and(|failure| failure.entry != Some(entry))
    });
    assert_eq!(
        unsupported.failure.unwrap().error.to_string(),
        "Opus audio in Ogg files is not supported"
    );

    let good = fixture("flac/s24_48000.flac");
    harness.play(&good);
    harness.wait_transport(TransportState::Playing);
    let ended = harness.wait_transport(TransportState::Stopped);
    assert!(ended.failure.is_none());
    assert!(harness.capture.epoch(ended.playback.generation.get()) == decoded(&good));
}

#[test]
fn unavailable_and_lost_devices_block_with_an_error() {
    let mut harness = Harness::new(48_000);
    *harness.capture.open_error.lock().unwrap() = Some(OutputError::NoDevice);
    harness.play(&fixture("flac/s24_48000.flac"));
    let blocked = harness.wait_transport(TransportState::Blocked);
    assert_eq!(
        blocked.failure.unwrap().error,
        PlaybackError::Output(OutputError::NoDevice)
    );

    *harness.capture.open_error.lock().unwrap() = None;
    let dir = tempfile::tempdir().unwrap();
    let long = counting_wav(dir.path(), "long.wav", 8, 0);
    harness.play(&long);
    let playing = harness.wait_transport(TransportState::Playing);
    let device = playing.output.unwrap().device;

    *harness.capture.lost.lock().unwrap() = Some(OutputError::DeviceLost);
    let lost = harness.wait_transport(TransportState::Blocked);
    assert_eq!(
        lost.failure.unwrap().error,
        PlaybackError::Output(OutputError::DeviceLost)
    );
    assert_eq!(harness.lost, [device]);
    assert!(lost.output.is_none());

    // Playing again opens a new stream.
    harness.engine.resume();
    harness.wait_transport(TransportState::Playing);
    assert_eq!(*harness.capture.opens.lock().unwrap(), 2);
}

#[test]
fn an_idle_stream_closes_and_resume_continues_from_the_paused_frame() {
    let dir = tempfile::tempdir().unwrap();
    let path = counting_wav(dir.path(), "long.wav", 8, 0);
    let mut harness = Harness::new(48_000);
    harness.play(&path);
    let playing = harness.wait_transport(TransportState::Playing);
    while harness.capture.total_frames() < 24_000 {
        std::thread::sleep(Duration::from_millis(5));
    }
    harness.engine.pause();
    harness.wait_transport(TransportState::Paused);
    let closed = harness.wait("stream closed", |snapshot| snapshot.output.is_none());
    assert_eq!(closed.playback.transport, TransportState::Paused);
    let played = harness
        .capture
        .epoch(playing.playback.generation.get())
        .len() as u64
        / 2;
    assert_eq!(closed.playback.position.unwrap().frame, played);

    harness.engine.resume();
    let resumed = harness.wait("resumed", |snapshot| {
        snapshot.playback.transport == TransportState::Playing && snapshot.output.is_some()
    });
    assert_eq!(*harness.capture.opens.lock().unwrap(), 2);
    let generation = resumed.playback.generation.get();
    while harness.capture.epoch(generation).len() < 2 * 4800 {
        std::thread::sleep(Duration::from_millis(5));
    }
    let frames: Vec<u64> = harness
        .capture
        .epoch(generation)
        .iter()
        .step_by(2)
        .map(|sample| counting_frame(*sample))
        .collect();
    let expected: Vec<u64> = (played..played + frames.len() as u64)
        .map(|frame| frame % 32_768)
        .collect();
    assert_eq!(frames, expected);
}

#[test]
fn back_to_back_seeks_land_on_the_last_target() {
    let dir = tempfile::tempdir().unwrap();
    let path = counting_wav(dir.path(), "long.wav", 8, 0);
    let mut harness = Harness::new(48_000);
    let entry = harness.play(&path);
    harness.wait_transport(TransportState::Playing);

    for round in 0..3u64 {
        // Two seeks inside one device period: the first is superseded before its flush.
        harness.engine.seek(entry, Duration::from_secs(1 + round));
        harness.engine.seek(entry, Duration::from_secs(4 + round));
        let target = (4 + round) * 48_000;
        let seeked = harness.wait("second seek playing", |snapshot| {
            snapshot.playback.transport == TransportState::Playing
                && snapshot
                    .playback
                    .position
                    .is_some_and(|position| position.frame >= target)
        });
        let generation = seeked.playback.generation.get();
        while harness.capture.epoch(generation).len() < 2 * 4800 {
            std::thread::sleep(Duration::from_millis(5));
        }
        let first = counting_frame(harness.capture.epoch(generation)[0]);
        assert_eq!(first, target % 32_768, "round {round}");
    }
}

#[test]
fn seek_and_play_after_a_decode_failure_reopen_the_file() {
    let dir = tempfile::tempdir().unwrap();
    let bytes = std::fs::read(fixture("flac/s24_48000.flac")).unwrap();
    let mut corrupt = bytes.clone();
    let middle = corrupt.len() * 3 / 4;
    for byte in &mut corrupt[middle..middle + 64] {
        *byte ^= 0x5a;
    }
    let path = dir.path().join("corrupt.flac");
    std::fs::write(&path, &corrupt).unwrap();
    // What decodes before the damage.
    let mut decoder = Decoder::open(&path).unwrap();
    let mut good = Vec::new();
    while let Ok(frames) = decoder.next_block(&mut good) {
        assert!(frames > 0, "the damaged fixture must fail");
    }
    let good: Vec<f32> = good.into_iter().map(|sample| sample as f32).collect();

    let mut harness = Harness::new(48_000);
    let entry = harness.play(&path);
    let failed = harness.wait("failure", |snapshot| snapshot.failure.is_some());
    assert_eq!(failed.playback.transport, TransportState::Stopped);

    harness.engine.seek(entry, Duration::from_millis(20));
    harness.engine.resume();
    let again = harness.wait("second failure", |snapshot| {
        snapshot.failure.is_some() && snapshot.playback.generation > failed.playback.generation
    });
    // The file was opened again rather than left hanging in Loading. Whatever played before the
    // damage stopped it again starts exactly at the seek target, with no stale audio.
    assert_eq!(again.playback.transport, TransportState::Stopped);
    let captured = harness.capture.epoch(again.playback.generation.get());
    assert!(captured[..] == good[960 * 2..960 * 2 + captured.len()]);
}

#[test]
fn pause_while_loading_is_reported_and_holds() {
    let dir = tempfile::tempdir().unwrap();
    let path = counting_wav(dir.path(), "long.wav", 8, 0);
    let mut harness = Harness::new(48_000);
    harness.play(&path);
    harness.engine.pause();
    let paused = harness.wait_transport(TransportState::Paused);
    harness.drain_for(Duration::from_millis(200));
    assert_eq!(
        harness.latest.as_ref().unwrap().playback.transport,
        TransportState::Paused
    );
    assert!(
        harness
            .capture
            .epoch(paused.playback.generation.get())
            .is_empty()
    );

    harness.engine.resume();
    harness.wait_transport(TransportState::Playing);
    let generation = paused.playback.generation.get();
    while harness.capture.epoch(generation).len() < 2 * 4800 {
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(counting_frame(harness.capture.epoch(generation)[0]), 0);
}

#[test]
fn rejecting_a_source_stops_what_was_playing() {
    let dir = tempfile::tempdir().unwrap();
    let path = counting_wav(dir.path(), "long.wav", 8, 0);
    let mut harness = Harness::new(48_000);
    harness.play(&path);
    harness.wait_transport(TransportState::Playing);

    let entry = QueueEntryId::generate();
    let range = ResolvedSource::Local {
        file: MediaFileId::generate(),
        path: path.clone(),
        span: parabolica_domain::source::FrameSpan::Range(0..48_000),
    };
    harness.engine.play(entry, range);
    let failed = harness.wait("CUE rejection", |snapshot| snapshot.failure.is_some());
    assert_eq!(failed.failure.unwrap().error, PlaybackError::CueRange);
    assert_eq!(failed.playback.transport, TransportState::Stopped);
    assert_eq!(failed.playback.current, None);

    harness.drain_for(Duration::from_millis(100));
    let frames = harness.capture.total_frames();
    harness.drain_for(Duration::from_millis(150));
    assert_eq!(
        harness.capture.total_frames(),
        frames,
        "old audio kept playing"
    );
}

#[test]
fn command_acknowledgements_are_ordered_and_independent_of_current_entry_and_generation() {
    let (engine, events) = PlaybackEngine::start(Box::new(NoOutput));
    let entry = QueueEntryId::generate();
    engine.play(
        entry,
        ResolvedSource::local_file(MediaFileId::generate(), fixture("flac/s16_44100.flac")),
    );
    let play = engine.synchronize();
    engine.stop();
    let stop = engine.synchronize();
    assert_ne!(play, stop);
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut acknowledgements = Vec::new();
    while acknowledgements.len() < 2 {
        if let EngineEvent::Synchronized { request, snapshot } = events
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .unwrap()
        {
            acknowledgements.push((request, snapshot));
        }
    }
    assert_eq!(acknowledgements[0].0, play);
    assert_eq!(acknowledgements[0].1.playback.current, Some(entry));
    assert!(acknowledgements[0].1.output.is_none());
    assert_eq!(
        acknowledgements[0].1.failure.as_ref().unwrap().entry,
        Some(entry)
    );
    assert_eq!(acknowledgements[1].0, stop);
    assert_eq!(
        acknowledgements[1].1.playback.transport,
        TransportState::Stopped
    );
    assert!(acknowledgements[1].1.failure.is_none());
}
