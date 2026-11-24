use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command as Process, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use parabolica_audio::engine::{NoOutput, OutputError, OutputStream, StreamRenderer};
use parabolica_audio::realtime::RealtimeRender;
use parabolica_domain::format::{ChannelCount, OutputFormat, OutputSampleFormat, SampleRate};
use parabolica_domain::output::{
    BufferConfig, BufferOwner, NegotiatedOutput, OutputBackend, OutputDeviceId,
};
use parabolica_domain::provider::{ProviderId, ProviderItemId, ProviderItemRef};
use parabolica_domain::queue::QueueCommand;

use super::*;

fn wait(service: &QueueService, predicate: impl Fn(&QueueSnapshot) -> bool) -> Arc<QueueSnapshot> {
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        let snapshot = service.snapshot();
        if predicate(&snapshot) {
            return snapshot;
        }
        assert!(
            Instant::now() < deadline,
            "queue did not settle: {snapshot:?}"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn missing_provider() -> PlayableSource {
    PlayableSource::Provider(ProviderItemRef {
        provider: ProviderId::YouTubeMusic,
        item: ProviderItemId("future-provider".into()),
    })
}

#[test]
fn commands_with_no_output_preserve_ids_shuffle_and_external_paths_on_drop() {
    let dir = tempfile::tempdir().unwrap();
    let database = dir.path().join("app.db");
    let storage = Storage::open(&database).unwrap();
    let path = dir.path().join("曲.wav");
    write_wav(&path, 48_000);
    let (service, _events) = QueueService::start(Some(storage.clone()), Box::new(NoOutput));
    wait(&service, |state| state.ready);
    let ids = service.enqueue_files(vec![path.clone(), path.clone()], EnqueueMode::Now);
    wait(&service, |state| {
        state.entries.len() == 2 && state.transport == TransportState::Blocked
    });
    service.queue(QueueCommand::SetShuffle {
        seed: Some(u64::MAX - 2),
    });
    service.queue(QueueCommand::SetRepeat(RepeatMode::All));
    let state = wait(&service, |state| {
        state.shuffle_seed == Some(u64::MAX - 2) && state.repeat == RepeatMode::All
    });
    assert_eq!(state.current, Some(ids[0]));
    assert_ne!(ids[0], ids[1]);
    assert_eq!(state.entries[0].entry.source, state.entries[1].entry.source);
    assert!(
        state
            .entries
            .iter()
            .all(|entry| entry.details.unavailable_reason.is_none())
    );
    assert!(state.history.is_empty());
    drop(service);
    let saved = storage.load_session().unwrap().unwrap();
    assert_eq!(saved.queue.current, Some(ids[0]));
    assert_eq!(
        saved
            .queue
            .entries
            .iter()
            .map(|entry| entry.id)
            .collect::<Vec<_>>(),
        ids
    );
    drop(storage);
    let storage = Storage::open(database).unwrap();
    let (restored, _events) = QueueService::start(Some(storage), Box::new(NoOutput));
    let restored_state = wait(&restored, |state| {
        state.ready && state.entries.len() == 2 && state.shuffle_seed.is_some()
    });
    assert_eq!(
        restored_state
            .entries
            .iter()
            .map(|entry| entry.entry.id)
            .collect::<Vec<_>>(),
        state
            .entries
            .iter()
            .map(|entry| entry.entry.id)
            .collect::<Vec<_>>()
    );
    assert_eq!(restored_state.entries[0].details.path, Some(path));
}

#[test]
fn unavailable_retries_and_repeat_all_skip_are_bounded_and_persisted() {
    let dir = tempfile::tempdir().unwrap();
    let storage = Storage::open(dir.path().join("app.db")).unwrap();
    let (service, _events) = QueueService::start(Some(storage.clone()), Box::new(NoOutput));
    service.enqueue_sources(
        vec![missing_provider(), missing_provider()],
        EnqueueMode::Now,
    );
    let state = wait(&service, |state| {
        state.entries.len() == 2 && state.transport == TransportState::Blocked
    });
    let id = state.current.unwrap();
    for attempt in 1..=3 {
        service.retry(id);
        wait(&service, |state| {
            state
                .entries
                .iter()
                .any(|entry| entry.entry.id == id && entry.retry_attempts == attempt)
        });
    }
    service.retry(id);
    wait(&service, |state| {
        state
            .error
            .as_ref()
            .is_some_and(|error| error.contains("Retry limit"))
    });
    service.queue(QueueCommand::SetRepeat(RepeatMode::All));
    service.skip();
    let state = wait(&service, |state| {
        state.repeat == RepeatMode::All && state.current.is_none()
    });
    assert_eq!(state.entries.len(), 2);
    assert!(
        state
            .entries
            .iter()
            .all(|entry| entry.details.unavailable_reason.is_some())
    );
    drop(service);
    assert_eq!(
        storage
            .load_session()
            .unwrap()
            .unwrap()
            .unavailable
            .iter()
            .find(|entry| entry.entry == id)
            .unwrap()
            .retry_attempts,
        3
    );
}

#[test]
fn playlist_import_likes_play_and_safe_export_run_on_the_worker() {
    let dir = tempfile::tempdir().unwrap();
    let storage = Storage::open(dir.path().join("app.db")).unwrap();
    let import = dir.path().join("input.m3u8");
    std::fs::write(&import, "#EXTM3U\n曲.wav\n曲.wav\n").unwrap();
    let (service, _events) = QueueService::start(Some(storage), Box::new(NoOutput));
    service.import_m3u8(import, "Imported".into());
    let state = wait(&service, |state| state.playlists.len() == 1);
    let playlist = &state.playlists[0];
    let id = playlist.id;
    assert_ne!(playlist.entries[0].id, playlist.entries[1].id);
    service.set_local_like(playlist.entries[0].details.source.clone(), true);
    service.play_playlist(id, EnqueueMode::Now);
    wait(&service, |state| {
        state.likes.len() == 1 && state.entries.len() == 2
    });
    let export = dir.path().join("output.m3u8");
    service.export_m3u8(id, export.clone());
    wait(&service, |state| state.last_export.is_some());
    assert_eq!(
        std::fs::read_to_string(&export).unwrap(),
        "#EXTM3U\n曲.wav\n曲.wav\n"
    );
    service.append_playlist(id, vec![missing_provider()]);
    wait(&service, |state| state.playlists[0].entries.len() == 3);
    let blocked = dir.path().join("blocked.m3u8");
    service.export_m3u8(id, blocked.clone());
    let state = wait(&service, |state| {
        state
            .last_export
            .as_ref()
            .is_some_and(|export| export.non_exportable.len() == 1)
    });
    assert!(
        state
            .error
            .as_ref()
            .unwrap()
            .contains("no file was written")
    );
    assert!(!blocked.exists());
}

#[derive(Debug, Clone, Default)]
pub(crate) struct ClockOutput {
    failure: Arc<Mutex<Option<OutputError>>>,
}

impl Output for ClockOutput {
    fn open(&mut self) -> Result<Box<dyn OutputStream>, OutputError> {
        Ok(Box::new(ClockStream {
            negotiated: NegotiatedOutput {
                device: OutputDeviceId {
                    backend: OutputBackend::WasapiShared,
                    stable_id: "queue-test".into(),
                },
                device_name: "Queue test clock".into(),
                format: OutputFormat {
                    sample: OutputSampleFormat::Float { bits: 32 },
                    rate: SampleRate::new(48_000).unwrap(),
                    channels: ChannelCount::STEREO,
                },
                channel_map: vec![0, 1],
                buffer: BufferConfig {
                    frames: 256,
                    owner: BufferOwner::App,
                },
                reported_latency: None,
            },
            stop: Arc::new(AtomicBool::new(false)),
            failure: self.failure.clone(),
            thread: None,
        }))
    }
}

struct ClockStream {
    negotiated: NegotiatedOutput,
    stop: Arc<AtomicBool>,
    failure: Arc<Mutex<Option<OutputError>>>,
    thread: Option<JoinHandle<()>>,
}

impl OutputStream for ClockStream {
    fn negotiated(&self) -> &NegotiatedOutput {
        &self.negotiated
    }
    fn start(&mut self, mut renderer: StreamRenderer) -> Result<(), OutputError> {
        let stop = self.stop.clone();
        self.thread = Some(std::thread::spawn(move || {
            let mut samples = [0.0_f32; 512];
            while !stop.load(Ordering::Acquire) {
                renderer.render(&mut samples, 2);
                std::thread::sleep(Duration::from_millis(2));
            }
        }));
        Ok(())
    }
    fn failure(&self) -> Option<OutputError> {
        self.failure.lock().unwrap().clone()
    }
}

impl Drop for ClockStream {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            thread.join().unwrap();
        }
    }
}

pub(crate) fn write_wav(path: &Path, frames: u32) {
    let bytes = frames * 4;
    let mut data = Vec::new();
    data.extend_from_slice(b"RIFF");
    data.extend_from_slice(&(36 + bytes).to_le_bytes());
    data.extend_from_slice(b"WAVEfmt ");
    data.extend_from_slice(&16_u32.to_le_bytes());
    data.extend_from_slice(&1_u16.to_le_bytes());
    data.extend_from_slice(&2_u16.to_le_bytes());
    data.extend_from_slice(&48_000_u32.to_le_bytes());
    data.extend_from_slice(&192_000_u32.to_le_bytes());
    data.extend_from_slice(&4_u16.to_le_bytes());
    data.extend_from_slice(&16_u16.to_le_bytes());
    data.extend_from_slice(b"data");
    data.extend_from_slice(&bytes.to_le_bytes());
    data.resize(data.len() + bytes as usize, 0);
    std::fs::write(path, data).unwrap();
}

#[test]
fn playing_removal_selects_successor_and_history_contains_actual_starts_only() {
    let dir = tempfile::tempdir().unwrap();
    let paths: Vec<_> = (0..3)
        .map(|index| dir.path().join(format!("{index}.wav")))
        .collect();
    for path in &paths {
        write_wav(path, 480_000);
    }
    let storage = Storage::open(dir.path().join("app.db")).unwrap();
    let (service, _events) = QueueService::start(Some(storage), Box::new(ClockOutput::default()));
    let ids = service.enqueue_files(paths, EnqueueMode::Now);
    wait(&service, |state| {
        state.transport == TransportState::Playing && state.history.len() == 1
    });
    service.queue(QueueCommand::Remove(ids[0]));
    let state = wait(&service, |state| {
        state.current == Some(ids[1]) && state.history.len() == 2
    });
    assert_eq!(
        state
            .history
            .iter()
            .map(|entry| entry.record.entry)
            .collect::<Vec<_>>(),
        ids[..2]
    );
    assert_eq!(state.entries.len(), 2);
    service.queue(QueueCommand::Jump(ids[2]));
    wait(&service, |state| {
        state.current == Some(ids[2]) && state.history.len() == 3
    });
    service.previous();
    wait(&service, |state| {
        state.current == Some(ids[1]) && state.history.len() == 4
    });
}

#[test]
fn clean_restart_restores_paused_exact_position_order_repeat_and_ids() {
    let dir = tempfile::tempdir().unwrap();
    let database = dir.path().join("app.db");
    let path = dir.path().join("long.wav");
    write_wav(&path, 480_000);
    let storage = Storage::open(&database).unwrap();
    let (service, _events) =
        QueueService::start(Some(storage.clone()), Box::new(ClockOutput::default()));
    let ids = service.enqueue_files(vec![path.clone(), path], EnqueueMode::Now);
    wait(&service, |state| {
        state.transport == TransportState::Playing && state.generation.is_some()
    });
    service.pause();
    wait(&service, |state| state.transport == TransportState::Paused);
    service.seek(ids[0], Duration::from_millis(1234));
    wait(&service, |state| {
        state.transport == TransportState::Paused && state.position == Duration::from_millis(1234)
    });
    service.queue(QueueCommand::SetShuffle { seed: Some(42) });
    service.queue(QueueCommand::SetRepeat(RepeatMode::One));
    wait(&service, |state| {
        state.shuffle_seed == Some(42)
            && state.repeat == RepeatMode::One
            && state.transport == TransportState::Paused
    });
    drop(service);
    let saved = storage.load_session().unwrap().unwrap();
    assert_eq!(saved.queue.position, Duration::from_millis(1234));
    drop(storage);
    let storage = Storage::open(&database).unwrap();
    let (service, _events) = QueueService::start(Some(storage), Box::new(ClockOutput::default()));
    let state = wait(&service, |state| {
        state.ready
            && state.transport == TransportState::Paused
            && state.position == Duration::from_millis(1234)
    });
    assert_eq!(state.current, Some(ids[0]));
    assert_eq!(state.history.len(), saved.history.len());
    assert_eq!(
        state
            .entries
            .iter()
            .map(|entry| entry.entry.id)
            .collect::<Vec<_>>(),
        saved.queue.shuffle.unwrap().order
    );
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(service.snapshot().position, Duration::from_millis(1234));
}

const WRITER_DB: &str = "PARABOLICA_QUEUE_WRITER_DB";

#[test]
fn writer_process() {
    let Ok(database) = std::env::var(WRITER_DB) else {
        return;
    };
    let database = PathBuf::from(database);
    let path = database.with_extension("wav");
    write_wav(&path, 4_800_000);
    let storage = Storage::open(&database).unwrap();
    let (service, _events) =
        QueueService::start(Some(storage.clone()), Box::new(ClockOutput::default()));
    let ids = service.enqueue_files(vec![path.clone(), path], EnqueueMode::Now);
    wait(&service, |state| state.transport == TransportState::Playing);
    service.queue(QueueCommand::SetShuffle { seed: Some(99) });
    service.queue(QueueCommand::SetRepeat(RepeatMode::All));
    wait(&service, |state| {
        state.repeat == RepeatMode::All && state.shuffle_seed == Some(99)
    });
    loop {
        let saved = storage.load_session().unwrap().unwrap();
        if saved.queue.position >= Duration::from_millis(500) {
            println!("checkpoint {} {}", ids[0], saved.queue.position.as_micros());
            std::io::Write::flush(&mut std::io::stdout()).unwrap();
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    loop {
        std::thread::park();
    }
}

#[test]
fn forced_termination_restores_the_last_committed_session_paused() {
    let dir = tempfile::tempdir().unwrap();
    let database = dir.path().join("crash.db");
    let mut child = Process::new(std::env::current_exe().unwrap())
        .args([
            "queue::tests::writer_process",
            "--nocapture",
            "--test-threads=1",
        ])
        .env(WRITER_DB, &database)
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
    let stdout = child.stdout.take().unwrap();
    let (reports, receiver) = mpsc::channel();
    let reader = std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            let line = line.unwrap();
            if let Some((_, value)) = line.split_once("checkpoint ") {
                let mut parts = value.split_whitespace();
                let entry = parts.next().unwrap().parse::<QueueEntryId>().unwrap();
                let position = parts.next().unwrap().parse::<u64>().unwrap();
                reports.send((entry, position)).unwrap();
                break;
            }
        }
    });
    let reported = receiver.recv_timeout(Duration::from_secs(15));
    child.kill().unwrap();
    child.wait().unwrap();
    reader.join().unwrap();
    let (entry, position) = reported.expect("child committed a queue checkpoint");
    let storage = Storage::open(&database).unwrap();
    let saved = storage.load_session().unwrap().unwrap();
    assert_eq!(saved.queue.current, Some(entry));
    assert!(saved.queue.position >= Duration::from_micros(position));
    assert_eq!(saved.queue.entries.len(), 2);
    let (service, _events) = QueueService::start(Some(storage), Box::new(ClockOutput::default()));
    let state = wait(&service, |state| {
        state.ready
            && state.transport == TransportState::Paused
            && state.entries.len() == 2
            && state.generation.is_some()
    });
    assert_eq!(state.current, Some(entry));
    let rate = SampleRate::new(48_000).unwrap();
    let frame = parabolica_audio::decode::duration_to_frames(saved.queue.position, rate);
    assert_eq!(
        state.position,
        Duration::from_secs_f64(frame as f64 / f64::from(rate.hz()))
    );
    assert!(state.position.abs_diff(saved.queue.position) < Duration::from_secs_f64(1. / 48_000.));
    assert_eq!(
        state
            .entries
            .iter()
            .map(|entry| entry.entry.id)
            .collect::<Vec<_>>(),
        saved.queue.shuffle.unwrap().order
    );
    assert_eq!(state.repeat, RepeatMode::All);
}

#[test]
fn every_short_track_boundary_is_recorded_before_event_coalescing() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("short.wav");
    write_wav(&path, 1024);
    let storage = Storage::open(dir.path().join("app.db")).unwrap();
    let (service, _events) = QueueService::start(Some(storage), Box::new(ClockOutput::default()));
    let ids = service.enqueue_files(vec![path.clone(), path.clone(), path], EnqueueMode::Now);
    let state = wait(&service, |state| {
        state.history.len() == 3 && state.current.is_none()
    });
    assert_eq!(
        state
            .history
            .iter()
            .map(|item| item.record.entry)
            .collect::<Vec<_>>(),
        ids
    );
    assert_eq!(state.transport, TransportState::Stopped);
}

#[test]
fn repeat_one_uses_real_boundaries_and_explicit_next_advances() {
    let dir = tempfile::tempdir().unwrap();
    let short = dir.path().join("short.wav");
    let long = dir.path().join("long.wav");
    write_wav(&short, 4096);
    write_wav(&long, 480_000);
    let (service, _events) = QueueService::start(None, Box::new(ClockOutput::default()));
    service.set_repeat(RepeatMode::One);
    let ids = service.enqueue_files(vec![short, long], EnqueueMode::Now);
    let state = wait(&service, |state| state.history.len() >= 3);
    assert!(state.history.iter().all(|item| item.record.entry == ids[0]));
    service.next();
    wait(&service, |state| {
        state.current == Some(ids[1])
            && state
                .history
                .last()
                .is_some_and(|item| item.record.entry == ids[1])
    });
}

#[test]
fn a_corrupt_upcoming_entry_is_selected_visibly_without_a_false_history_record() {
    let dir = tempfile::tempdir().unwrap();
    let first = dir.path().join("first.wav");
    let corrupt = dir.path().join("corrupt.wav");
    let third = dir.path().join("third.wav");
    write_wav(&first, 4800);
    std::fs::write(&corrupt, b"not audio").unwrap();
    write_wav(&third, 480_000);
    let storage = Storage::open(dir.path().join("app.db")).unwrap();
    let (service, _events) = QueueService::start(Some(storage), Box::new(ClockOutput::default()));
    let ids = service.enqueue_files(vec![first, corrupt, third], EnqueueMode::Now);
    let state = wait(&service, |state| {
        state.current == Some(ids[1]) && state.transport == TransportState::Blocked
    });
    assert_eq!(
        state
            .history
            .iter()
            .map(|item| item.record.entry)
            .collect::<Vec<_>>(),
        vec![ids[0]]
    );
    assert!(state.entries[1].details.unavailable_reason.is_some());
    service.skip();
    wait(&service, |state| {
        state.current == Some(ids[2]) && state.history.len() == 2
    });
}

#[test]
fn output_loss_does_not_mark_the_source_unavailable() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("long.wav");
    write_wav(&path, 480_000);
    let output = ClockOutput::default();
    let failure = output.failure.clone();
    let (service, _events) = QueueService::start(None, Box::new(output));
    let ids = service.enqueue_files(vec![path], EnqueueMode::Now);
    wait(&service, |state| state.transport == TransportState::Playing);
    *failure.lock().unwrap() = Some(OutputError::DeviceLost);
    let state = wait(&service, |state| state.transport == TransportState::Blocked);
    assert_eq!(state.current, Some(ids[0]));
    assert!(state.entries[0].details.unavailable_reason.is_none());
    assert_eq!(state.entries[0].retry_attempts, 0);
}

#[test]
fn pause_resume_preserves_the_position_and_does_not_add_a_listening_start() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("long.wav");
    write_wav(&path, 480_000);
    let (service, _) = QueueService::start(None, Box::new(ClockOutput::default()));
    let ids = service.enqueue_files(vec![path], EnqueueMode::Now);
    wait(&service, |state| {
        state.transport == TransportState::Playing && state.history.len() == 1
    });
    service.pause();
    wait(&service, |state| state.transport == TransportState::Paused);
    service.seek(ids[0], Duration::from_secs(2));
    wait(&service, |state| {
        state.transport == TransportState::Paused && state.position == Duration::from_secs(2)
    });
    service.resume();
    let resumed = wait(&service, |state| state.transport == TransportState::Playing);
    assert!(resumed.position >= Duration::from_secs(2));
    assert_eq!(resumed.history.len(), 1);
}

#[test]
fn bulk_likes_save_more_than_an_inbox_of_sources_as_one_command() {
    let dir = tempfile::tempdir().unwrap();
    let storage = Storage::open(dir.path().join("app.db")).unwrap();
    let (service, _) = QueueService::start(Some(storage.clone()), Box::new(NoOutput));
    let sources: Vec<_> = (0..256)
        .map(|index| {
            PlayableSource::Provider(ProviderItemRef {
                provider: ProviderId::YouTubeMusic,
                item: ProviderItemId(format!("item-{index}")),
            })
        })
        .collect();
    assert!(service.set_local_likes(sources, true));
    wait(&service, |state| state.likes.len() == 256);
    assert_eq!(storage.local_likes().unwrap().len(), 256);
}

#[test]
fn rejected_commands_remain_visible_across_worker_publications_until_dismissed() {
    struct GatedOutput(Option<mpsc::Receiver<()>>);
    impl Output for GatedOutput {
        fn open(&mut self) -> Result<Box<dyn OutputStream>, OutputError> {
            if let Some(gate) = self.0.take() {
                gate.recv_timeout(Duration::from_secs(5)).unwrap();
            }
            Err(OutputError::Unavailable)
        }
    }
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("track.wav");
    write_wav(&path, 4800);
    let (release, gate) = mpsc::channel();
    let (service, _) = QueueService::start(None, Box::new(GatedOutput(Some(gate))));
    service.enqueue_files(vec![path], EnqueueMode::Now);
    wait(&service, |state| state.transport == TransportState::Loading);
    for _ in 0..64 {
        assert!(service.set_repeat(RepeatMode::All));
    }
    assert!(!service.set_repeat(RepeatMode::One));
    let revision = service.snapshot().revision;
    wait(&service, |state| {
        state.revision > revision
            && state
                .error
                .as_ref()
                .is_some_and(|error| error.contains("busy"))
    });
    release.send(()).unwrap();
    wait(&service, |state| {
        state.repeat == RepeatMode::All && state.transport == TransportState::Blocked
    });
    assert!(service.snapshot().error.as_ref().unwrap().contains("busy"));
    service.dismiss_error();
    wait(&service, |state| state.error.is_none());
}

#[test]
fn worker_errors_stay_dismissed_across_publications() {
    let dir = tempfile::tempdir().unwrap();
    let storage = Storage::open(dir.path().join("app.db")).unwrap();
    let path = dir.path().join("invalid.m3u8");
    std::fs::write(&path, "#EXTM3U\nhttps://example.invalid/track.flac\n").unwrap();
    let (service, _) = QueueService::start(Some(storage), Box::new(NoOutput));
    wait(&service, |state| state.ready);
    service.import_m3u8(path.clone(), "Invalid".into());
    let failed = wait(&service, |state| state.error.is_some());
    let error = failed.error.clone();
    service.dismiss_error();
    let dismissed = service.snapshot().revision;
    let published = wait(&service, |state| state.revision > dismissed + 2);
    assert!(published.error.is_none(), "{:?}", published.error);
    service.import_m3u8(path, "Invalid again".into());
    let failed_again = wait(&service, |state| state.error.is_some());
    assert_eq!(failed_again.error, error);
    service.shutdown();
}

#[test]
fn one_frame_repeat_with_sqlite_cannot_starve_pause_clear_or_shutdown() {
    let dir = tempfile::tempdir().unwrap();
    let storage = Storage::open(dir.path().join("app.db")).unwrap();
    let path = dir.path().join("one-frame.wav");
    write_wav(&path, 1);
    let (service, _events) =
        QueueService::start(Some(storage.clone()), Box::new(ClockOutput::default()));
    service.set_repeat(RepeatMode::One);
    service.enqueue_files(vec![path.clone()], EnqueueMode::Now);
    wait(&service, |state| state.history.len() >= 100);
    let pause_at = Instant::now();
    service.pause();
    wait(&service, |state| state.transport == TransportState::Paused);
    assert!(pause_at.elapsed() < Duration::from_secs(2));
    assert!(service.snapshot().history.len() <= parabolica_storage::session::HISTORY_LIMIT);
    service.resume();
    wait(&service, |state| state.transport == TransportState::Playing);
    let clear_at = Instant::now();
    service.clear();
    wait(&service, |state| {
        state.entries.is_empty() && state.transport == TransportState::Stopped
    });
    assert!(clear_at.elapsed() < Duration::from_secs(2));
    service.enqueue_files(vec![path], EnqueueMode::Now);
    wait(&service, |state| state.transport == TransportState::Playing);
    std::thread::sleep(Duration::from_millis(20));
    let shutdown_at = Instant::now();
    service.shutdown();
    assert!(shutdown_at.elapsed() < Duration::from_secs(2));
    let saved = storage.load_session().unwrap().unwrap();
    assert_eq!(saved.queue.entries.len(), 1);
    assert!(saved.queue.current.is_some());
}

#[test]
fn output_changes_pause_and_persist_the_selection_without_losing_queue_position() {
    use crate::ui::listening_device::preferences::SavedOutput;
    use parabolica_domain::output::SampleRatePolicy;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("music.wav");
    write_wav(&path, 480_000);
    let storage = Storage::open(dir.path().join("app.db")).unwrap();
    let (service, _) = QueueService::start(Some(storage.clone()), Box::new(ClockOutput::default()));
    service.enqueue_files(vec![path], EnqueueMode::Now);
    wait(&service, |state| {
        state.transport == TransportState::Playing && state.position > Duration::from_millis(50)
    });
    let selection = OutputSelection {
        backend: OutputBackend::WasapiExclusive,
        device: Some(OutputDeviceId {
            backend: OutputBackend::WasapiExclusive,
            stable_id: "selected-headphones".into(),
        }),
        rate: SampleRatePolicy::Fixed(SampleRate::new(96_000).unwrap()),
        asio: Default::default(),
    };
    service.select_output(selection.clone());
    let paused = wait(&service, |state| {
        state.output_selection == selection && state.transport == TransportState::Paused
    });
    assert!(paused.position > Duration::ZERO);
    assert!(paused.history.len() == 1);
    service.shutdown();
    assert_eq!(
        storage.preference::<SavedOutput>().unwrap(),
        Some(SavedOutput(selection.clone()))
    );
    let (restored, _) = QueueService::start(Some(storage), Box::new(ClockOutput::default()));
    let state = wait(&restored, |state| {
        state.ready && state.transport == TransportState::Paused
    });
    assert_eq!(state.output_selection, selection);
    assert_eq!(state.current, paused.current);
    assert!(state.position.abs_diff(paused.position) <= Duration::from_secs_f64(1.0 / 48_000.0));
}

#[test]
fn an_invalid_saved_output_requires_a_new_selection_before_playback() {
    use parabolica_storage::preferences::Preference;
    struct InvalidOutput;
    impl Preference for InvalidOutput {
        const KEY: &'static str = "audio.output";
        fn encode(&self) -> String {
            "future incompatible setting".into()
        }
        fn decode(_: &str) -> Option<Self> {
            None
        }
    }
    let dir = tempfile::tempdir().unwrap();
    let storage = Storage::open(dir.path().join("app.db")).unwrap();
    storage.save_preference(&InvalidOutput).unwrap();
    let path = dir.path().join("music.wav");
    write_wav(&path, 480_000);
    let (service, _) = QueueService::start(Some(storage), Box::new(ClockOutput::default()));
    wait(&service, |state| {
        state.ready && state.output_settings_error.is_some()
    });
    service.enqueue_files(vec![path], EnqueueMode::Now);
    let state = wait(&service, |state| {
        state
            .error
            .as_ref()
            .is_some_and(|error| error.contains("Choose an output"))
    });
    assert_ne!(state.transport, TransportState::Playing);
    service.select_output(OutputSelection::default());
    wait(&service, |state| state.output_settings_error.is_none());
    service.resume();
    wait(&service, |state| state.transport == TransportState::Playing);
}

#[test]
fn an_output_retry_records_the_first_start_once_and_loss_recovery_does_not_add_a_start() {
    struct BusyOutput {
        busy: Arc<AtomicBool>,
        clock: ClockOutput,
    }
    impl Output for BusyOutput {
        fn open(&mut self) -> Result<Box<dyn OutputStream>, OutputError> {
            if self.busy.load(Ordering::Acquire) {
                Err(OutputError::Busy)
            } else {
                self.clock.open()
            }
        }
    }
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("music.wav");
    write_wav(&path, 480_000);
    let storage = Storage::open(dir.path().join("app.db")).unwrap();
    let busy = Arc::new(AtomicBool::new(true));
    let clock = ClockOutput::default();
    let (service, _) = QueueService::start(
        Some(storage),
        Box::new(BusyOutput {
            busy: busy.clone(),
            clock: clock.clone(),
        }),
    );
    let id = service.enqueue_files(vec![path], EnqueueMode::Now)[0];
    let blocked = wait(&service, |state| state.transport == TransportState::Blocked);
    assert!(blocked.history.is_empty());
    busy.store(false, Ordering::Release);
    service.resume();
    let playing = wait(&service, |state| state.transport == TransportState::Playing);
    assert_eq!(playing.history.len(), 1);
    assert_eq!(playing.history[0].record.entry, id);
    *clock.failure.lock().unwrap() = Some(OutputError::DeviceLost);
    wait(&service, |state| state.transport == TransportState::Blocked);
    *clock.failure.lock().unwrap() = None;
    service.resume();
    let recovered = wait(&service, |state| state.transport == TransportState::Playing);
    assert_eq!(recovered.history.len(), 1);
}
