use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use gpui_kit::test::TestWindowExt as _;
use gpui_kit::{
    AnyWindowHandle, AppContext as _, Bounds, Entity, Point, TestAppContext, WindowBounds,
    WindowOptions, px, size,
};
use parabolica_audio::decode::{BasicTags, Decoder};
use parabolica_audio::engine::{EngineSnapshot, Output, OutputError, OutputStream, StreamRenderer};
use parabolica_audio::realtime::RealtimeRender as _;
use parabolica_domain::control::GenerationCounter;
use parabolica_domain::format::{ChannelCount, OutputFormat, OutputSampleFormat, SampleRate};
use parabolica_domain::gain::Volume;
use parabolica_domain::ids::{MediaFileId, QueueEntryId};
use parabolica_domain::output::{
    BufferConfig, BufferOwner, NegotiatedOutput, OutputBackend, OutputDeviceId,
};
use parabolica_domain::playback::{PlaybackSnapshot, TransportState};
use parabolica_domain::provider::{ProviderId, ProviderItemId, ProviderItemRef};
use parabolica_domain::queue::QueueEntry;
use parabolica_domain::source::PlayableSource;
use parabolica_storage::Storage;

use super::open_request::{MAX_PATHS, RequestError};
use super::presentation::{Artwork, MediaAction, MediaStatus, Presentation, apply, present};
use super::presenter::{MediaSink, Presenter};
use super::*;
use crate::controllers::queue::{QueueEntryView, QueueSnapshot, SourceView};

fn root() -> PathBuf {
    if cfg!(windows) {
        PathBuf::from(r"C:\Música")
    } else {
        PathBuf::from("/música")
    }
}

/// Nested Unicode folders past Windows' 260-character `MAX_PATH`.
fn long_unicode_dir(base: &Path) -> PathBuf {
    let mut path = base.to_path_buf();
    while path.as_os_str().len() < 320 {
        path.push("曲集 — Música ナイトフライト");
    }
    path
}

fn write_wav(path: &Path, frames: u32) {
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
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, data).unwrap();
}

fn wait(queue: &QueueService, predicate: impl Fn(&QueueSnapshot) -> bool) -> Arc<QueueSnapshot> {
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        let snapshot = queue.snapshot();
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

/// A device that consumes audio in real time without a sound card.
#[derive(Debug, Default)]
struct ClockOutput;

impl Output for ClockOutput {
    fn open(&mut self) -> Result<Box<dyn OutputStream>, OutputError> {
        Ok(Box::new(ClockStream {
            negotiated: NegotiatedOutput {
                device: OutputDeviceId {
                    backend: OutputBackend::WasapiShared,
                    stable_id: "os-events-test".into(),
                },
                device_name: "OS events test clock".into(),
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
            thread: None,
        }))
    }
}

struct ClockStream {
    negotiated: NegotiatedOutput,
    stop: Arc<AtomicBool>,
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
        None
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

/// Raw request bytes, bypassing `encode`'s checks.
fn raw_request(paths: &[&str]) -> Vec<u8> {
    let mut bytes = b"PBLOPEN1".to_vec();
    bytes.extend((paths.len() as u32).to_le_bytes());
    for path in paths {
        bytes.extend((path.len() as u32).to_le_bytes());
        bytes.extend(path.as_bytes());
    }
    bytes
}

#[test]
fn open_requests_round_trip_unicode_and_long_paths_in_order() {
    let long = long_unicode_dir(&root()).join("曲.flac");
    assert!(long.as_os_str().len() > 260);
    let request = OpenRequest {
        paths: vec![root().join("ナイト.flac"), long, root().join("ナイト.flac")],
    };
    assert_eq!(OpenRequest::decode(&request.encode()), Ok(request));
    assert_eq!(
        OpenRequest::decode(&OpenRequest::default().encode()),
        Ok(OpenRequest::default())
    );
}

#[test]
fn command_line_files_resolve_against_the_working_directory() {
    let absolute = root().join("b.flac");
    let mut args = vec![
        OsString::from("a.flac"),
        absolute.clone().into_os_string(),
        OsString::from("line\nbreak.flac"),
    ];
    // Not Unicode: a stray byte on Unix, an unpaired surrogate on Windows.
    #[cfg(unix)]
    args.push(std::os::unix::ffi::OsStringExt::from_vec(vec![0xff, b'x']));
    #[cfg(windows)]
    args.push(std::os::windows::ffi::OsStringExt::from_wide(&[
        0xD800,
        u16::from(b'x'),
    ]));
    let request = OpenRequest::from_args(args.clone(), Some(&root()));
    assert_eq!(request.paths, vec![root().join("a.flac"), absolute.clone()]);
    assert_eq!(
        OpenRequest::from_args(args, None).paths,
        vec![absolute],
        "a relative path without a working directory is dropped"
    );
}

#[test]
fn a_damaged_request_opens_nothing() {
    let good = root().join("曲.flac");
    let good = good.to_str().unwrap();
    let relative = if cfg!(windows) {
        r"Música\曲.flac"
    } else {
        "música/曲.flac"
    };
    let device = r"\\.\pipe\other";
    let mut trailing = raw_request(&[good]);
    trailing.push(0);
    let mut truncated = raw_request(&[good]);
    truncated.pop();
    let mut too_many = b"PBLOPEN1".to_vec();
    too_many.extend((MAX_PATHS as u32 + 1).to_le_bytes());

    assert_eq!(
        OpenRequest::decode(b"PBLOPEN0"),
        Err(RequestError::Malformed)
    );
    assert_eq!(OpenRequest::decode(&trailing), Err(RequestError::Malformed));
    assert_eq!(
        OpenRequest::decode(&truncated),
        Err(RequestError::InvalidPath)
    );
    assert_eq!(
        OpenRequest::decode(&too_many),
        Err(RequestError::TooManyPaths)
    );
    for path in [relative, &format!("{good}\0.flac"), &format!("{good}\n")] {
        assert_eq!(
            OpenRequest::decode(&raw_request(&[good, path])),
            Err(RequestError::InvalidPath),
            "{path:?}"
        );
    }
    if cfg!(windows) {
        assert_eq!(
            OpenRequest::decode(&raw_request(&[device])),
            Err(RequestError::InvalidPath)
        );
    }
}

fn local_entry(title: &str) -> QueueEntryView {
    QueueEntryView {
        entry: QueueEntry {
            id: QueueEntryId::generate(),
            source: PlayableSource::LocalFile(MediaFileId::generate()),
        },
        details: SourceView {
            source: PlayableSource::LocalFile(MediaFileId::generate()),
            title: title.into(),
            source_label: "Local file".into(),
            path: Some(root().join(format!("{title}.flac"))),
            unavailable_reason: None,
        },
        retry_attempts: 0,
    }
}

fn queue_with(
    entries: Vec<QueueEntryView>,
    current: usize,
    transport: TransportState,
) -> QueueSnapshot {
    QueueSnapshot {
        current: Some(entries[current].entry.id),
        entries,
        transport,
        ..QueueSnapshot::default()
    }
}

fn engine_playing(entry: QueueEntryId, title: &str) -> EngineSnapshot {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("tags.wav");
    write_wav(&path, 480);
    let mut source = Decoder::open(&path).unwrap().info().clone();
    source.tags = BasicTags {
        title: Some(title.into()),
        artist: Some(format!("{title} artist")),
        album: Some(format!("{title} album")),
    };
    EngineSnapshot {
        playback: PlaybackSnapshot {
            generation: GenerationCounter::default().advance(),
            transport: TransportState::Playing,
            current: Some(entry),
            position: None,
            duration: None,
            route: None,
            signal_path: None,
            gain: None,
            volume: Volume::new(1.).unwrap(),
        },
        path: Some(path),
        source: Some(source),
        output: None,
        underruns: 0,
        failure: None,
    }
}

#[test]
fn an_empty_queue_closes_the_session_and_a_queue_without_selection_offers_play() {
    let empty = present(&QueueSnapshot::default(), None, true);
    assert_eq!(empty, Presentation::CLOSED);

    let queued = QueueSnapshot {
        entries: vec![local_entry("First")],
        ..QueueSnapshot::default()
    };
    let presentation = present(&queued, None, true);
    assert_eq!(presentation.status, MediaStatus::Stopped);
    assert!(presentation.can_play && !presentation.can_pause);
    assert_eq!(presentation.details, None);
}

#[test]
fn details_follow_the_selected_entry_and_never_the_previous_tracks_tags() {
    let queue = queue_with(
        vec![local_entry("Previous"), local_entry("Selected")],
        1,
        TransportState::Loading,
    );
    // The engine still reports the previous entry while the selected one loads.
    let stale = engine_playing(queue.entries[0].entry.id, "Previous tag");
    let presentation = present(&queue, Some(&stale), true);
    let details = presentation.details.unwrap();
    assert_eq!(details.entry, queue.entries[1].entry.id);
    assert_eq!(details.title, "Selected");
    assert_eq!((details.artist.as_str(), details.album.as_str()), ("", ""));
    let PlayableSource::LocalFile(file) = queue.entries[1].entry.source else {
        unreachable!()
    };
    assert_eq!(
        details.artwork,
        Artwork::LocalFile {
            file,
            path: root().join("Selected.flac")
        }
    );

    let current = engine_playing(queue.entries[1].entry.id, "Selected tag");
    let details = present(&queue, Some(&current), true).details.unwrap();
    assert_eq!(details.title, "Selected tag");
    assert_eq!(details.artist, "Selected tag artist");
    assert_eq!(details.album, "Selected tag album");
}

#[test]
fn hidden_details_keep_the_playback_buttons() {
    let queue = queue_with(vec![local_entry("Private")], 0, TransportState::Playing);
    let engine = engine_playing(queue.entries[0].entry.id, "Private tag");
    let presentation = present(&queue, Some(&engine), false);
    assert_eq!(presentation.status, MediaStatus::Playing);
    assert_eq!(presentation.details, None);
    assert!(presentation.can_pause && presentation.can_next && presentation.can_previous);
    assert!(!presentation.can_play);
}

#[test]
fn provider_entries_show_their_title_without_artwork() {
    let mut entry = local_entry("Streaming title");
    entry.entry.source = PlayableSource::Provider(ProviderItemRef {
        provider: ProviderId::YouTubeMusic,
        item: ProviderItemId("video".into()),
    });
    entry.details.path = None;
    let queue = queue_with(vec![entry], 0, TransportState::Paused);
    let presentation = present(&queue, None, true);
    assert_eq!(presentation.status, MediaStatus::Paused);
    let details = presentation.details.unwrap();
    assert_eq!(details.title, "Streaming title");
    assert_eq!(details.artwork, Artwork::None);
}

type Shown = Arc<Mutex<Vec<(Option<QueueEntryId>, Option<Vec<u8>>)>>>;

struct Recorder(Shown);

impl MediaSink for Recorder {
    fn show(&mut self, presentation: &Presentation, artwork: Option<&[u8]>) {
        self.0.lock().unwrap().push((
            presentation.details.as_ref().map(|details| details.entry),
            artwork.map(<[u8]>::to_vec),
        ));
    }
}

fn wait_for(shown: &Shown, count: usize) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while shown.lock().unwrap().len() < count {
        assert!(
            Instant::now() < deadline,
            "shown: {:?}",
            shown.lock().unwrap()
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn slow_artwork_never_delays_changes_or_shows_on_a_replaced_entry() {
    let queue = queue_with(
        vec![local_entry("Slow art"), local_entry("Next")],
        0,
        TransportState::Playing,
    );
    let first = present(&queue, None, true);
    let second = present(
        &QueueSnapshot {
            current: Some(queue.entries[1].entry.id),
            ..queue.clone()
        },
        None,
        true,
    );
    let hidden = present(&queue, None, false);
    let slow_path = root().join("Slow art.flac");

    let (release, gate) = mpsc::channel::<()>();
    let shown = Shown::default();
    let presenter = Presenter::start(
        Recorder(shown.clone()),
        Box::new(move |artwork| {
            let Artwork::LocalFile { path, .. } = artwork else {
                return None;
            };
            if *path == slow_path {
                gate.recv().unwrap();
            }
            Some(Arc::new(path.to_string_lossy().into_owned().into_bytes()))
        }),
    );
    let (slow, next) = (queue.entries[0].entry.id, queue.entries[1].entry.id);
    presenter.present(first);
    wait_for(&shown, 1);
    // The first entry's artwork read is now blocked; later changes still apply at once.
    presenter.present(second.clone());
    wait_for(&shown, 2);
    presenter.present(hidden);
    wait_for(&shown, 3);
    release.send(()).unwrap();
    drop(presenter);
    assert_eq!(
        *shown.lock().unwrap(),
        [
            (Some(slow), None),
            (Some(next), None),
            (None, None),
            (None, None)
        ],
        "late artwork never shows after the entry changed or details were hidden, \
         and quitting clears the controls"
    );

    let shown = Shown::default();
    let presenter = Presenter::start(
        Recorder(shown.clone()),
        Box::new(|_| Some(Arc::new(b"cover".to_vec()))),
    );
    presenter.present(second);
    wait_for(&shown, 2);
    assert_eq!(
        shown.lock().unwrap()[..2],
        [(Some(next), None), (Some(next), Some(b"cover".to_vec()))]
    );
}

#[test]
fn media_buttons_send_the_players_queue_commands() {
    let dir = tempfile::tempdir().unwrap();
    let (first, second) = (dir.path().join("1.wav"), dir.path().join("2.wav"));
    write_wav(&first, 480_000);
    write_wav(&second, 480_000);
    let (queue, _events) = QueueService::start(None, Box::new(ClockOutput));
    wait(&queue, |state| state.ready);
    let ids = queue.enqueue_files(vec![first, second], EnqueueMode::Now);
    wait(&queue, |state| {
        state.current == Some(ids[0]) && state.transport == TransportState::Playing
    });

    apply(MediaAction::Pause, &queue);
    wait(&queue, |state| state.transport == TransportState::Paused);
    apply(MediaAction::Play, &queue);
    wait(&queue, |state| state.transport == TransportState::Playing);
    apply(MediaAction::Next, &queue);
    wait(&queue, |state| {
        state.current == Some(ids[1]) && state.transport == TransportState::Playing
    });
    apply(MediaAction::Previous, &queue);
    wait(&queue, |state| state.current == Some(ids[0]));
}

#[test]
fn sleep_pauses_and_saves_the_position_and_wake_does_not_resume() {
    let dir = tempfile::tempdir().unwrap();
    let storage = Storage::open(dir.path().join("app.db")).unwrap();
    let file = dir.path().join("曲.wav");
    write_wav(&file, 480_000);
    let (queue, _events) = QueueService::start(Some(storage.clone()), Box::new(ClockOutput));
    wait(&queue, |state| state.ready);
    let ids = queue.enqueue_files(vec![file], EnqueueMode::Now);
    wait(&queue, |state| state.transport == TransportState::Playing);
    std::thread::sleep(Duration::from_millis(300));

    pause_for_sleep(&queue, SLEEP_CHECKPOINT_WAIT);
    assert_eq!(queue.snapshot().transport, TransportState::Paused);
    let saved = storage.load_session().unwrap().unwrap();
    assert_eq!(saved.queue.current, Some(ids[0]));
    assert!(
        saved.queue.position >= Duration::from_millis(200),
        "{:?}",
        saved.queue.position
    );

    // The reported position may settle once after the pause fade; then it must stay put.
    std::thread::sleep(Duration::from_millis(300));
    let settled = queue.snapshot().position;
    queue.refresh_outputs();
    std::thread::sleep(Duration::from_millis(600));
    let woken = queue.snapshot();
    assert_eq!(woken.transport, TransportState::Paused);
    assert_eq!(woken.position, settled);
}

#[test]
fn a_hidden_track_detail_preference_round_trips_and_defaults_to_shown() {
    let dir = tempfile::tempdir().unwrap();
    let storage = Storage::open(dir.path().join("app.db")).unwrap();
    assert!(load_saved_sharing(Some(&storage)));
    assert!(load_saved_sharing(None));
    storage.save_preference(&ShareTrackDetails(false)).unwrap();
    assert!(!load_saved_sharing(Some(&storage)));
    assert_eq!(ShareTrackDetails::decode("on"), None);
}

fn open_attached_shell(
    cx: &mut TestAppContext,
    storage: Storage,
    share_details: bool,
) -> (AnyWindowHandle, Entity<Shell>, mpsc::Sender<OpenRequest>) {
    let (sender, requests) = mpsc::channel();
    let instance = Instance {
        requests: Some(requests),
        ..Instance::standalone(OpenRequest::default())
    };
    let (window, shell) = cx.update(|cx| {
        gpui_kit::init(cx);
        crate::ui::shell::init(cx);
        crate::ui::library::init(Some(storage.clone()), cx);
        init(instance, share_details, Some(storage.clone()), cx);
        crate::ui::appearance::init(None, Some(storage), cx);
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds {
                origin: Point::default(),
                size: size(px(1280.), px(800.)),
            })),
            ..Default::default()
        };
        let (window, shell) = gpui_kit::open_window(options, cx, |window, cx| {
            cx.new(|cx| Shell::new(None, window, cx))
        })
        .expect("open the shell window");
        attach(window, &shell, cx);
        (window, shell)
    });
    (window, shell, sender)
}

fn shell_queue(cx: &mut TestAppContext, shell: &Entity<Shell>) -> QueueService {
    cx.update(|cx| shell.read(cx).player().read(cx).queue())
}

/// Lets the queue worker's real threads and the UI's timers run until `predicate` holds.
fn settle(
    cx: &mut TestAppContext,
    queue: &QueueService,
    predicate: impl Fn(&QueueSnapshot) -> bool,
) -> Arc<QueueSnapshot> {
    for _ in 0..1600 {
        std::thread::sleep(Duration::from_millis(5));
        cx.executor().advance_clock(REQUEST_POLL);
        cx.run_until_parked();
        let snapshot = queue.snapshot();
        if predicate(&snapshot) {
            return snapshot;
        }
    }
    panic!("queue did not settle: {:?}", queue.snapshot());
}

#[gpui_kit::test]
fn a_second_launch_plays_through_the_running_player_and_quit_saves_the_session(
    cx: &mut TestAppContext,
) {
    let dir = tempfile::tempdir().unwrap();
    let storage = Storage::open(dir.path().join("app.db")).unwrap();
    let file = long_unicode_dir(dir.path()).join("曲.wav");
    write_wav(&file, 48_000);
    let (_window, shell, requests) = open_attached_shell(cx, storage.clone(), true);
    let queue = shell_queue(cx, &shell);
    settle(cx, &queue, |state| state.ready);

    requests
        .send(OpenRequest {
            paths: vec![file.clone()],
        })
        .unwrap();
    let state = settle(cx, &queue, |state| state.current.is_some());
    assert_eq!(state.entries.len(), 1);
    assert_eq!(
        state.entries[0].details.path.as_deref(),
        Some(file.as_path())
    );
    let entry = state.entries[0].entry.id;

    cx.quit();
    // Quit returns only after the worker saved its final checkpoint and exited.
    assert!(
        !queue.pause(),
        "the queue worker is still running after quit"
    );
    let saved = storage.load_session().unwrap().unwrap();
    assert_eq!(saved.queue.current, Some(entry));
    assert_eq!(
        saved
            .queue
            .entries
            .iter()
            .map(|entry| entry.id)
            .collect::<Vec<_>>(),
        vec![entry]
    );
}

#[gpui_kit::test]
fn the_open_dialog_keeps_unicode_long_paths_and_cancel_changes_nothing(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let storage = Storage::open(dir.path().join("app.db")).unwrap();
    let file = long_unicode_dir(dir.path()).join("ナイト.wav");
    write_wav(&file, 48_000);
    let (window, shell, _requests) = open_attached_shell(cx, storage, true);
    let queue = shell_queue(cx, &shell);
    settle(cx, &queue, |state| state.ready);

    let click_open = |cx: &mut TestAppContext| {
        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            window.click("open-file", cx);
        })
        .unwrap();
    };
    click_open(cx);
    cx.simulate_path_prompt_response({
        let file = file.clone();
        move |_| Some(vec![file])
    });
    let opened = settle(cx, &queue, |state| state.current.is_some());
    assert_eq!(
        opened.entries[0].details.path.as_deref(),
        Some(file.as_path())
    );

    click_open(cx);
    cx.simulate_path_prompt_response(|_| None);
    std::thread::sleep(Duration::from_millis(50));
    cx.run_until_parked();
    let after_cancel = queue.snapshot();
    assert_eq!(after_cancel.current, opened.current);
    assert_eq!(
        after_cancel
            .entries
            .iter()
            .map(|entry| entry.entry.id)
            .collect::<Vec<_>>(),
        opened
            .entries
            .iter()
            .map(|entry| entry.entry.id)
            .collect::<Vec<_>>()
    );
}

#[gpui_kit::test]
fn a_failure_in_the_background_notifies_without_track_details_when_hidden(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let storage = Storage::open(dir.path().join("app.db")).unwrap();
    let (window, _shell, _requests) = open_attached_shell(cx, storage, false);
    let background = cx
        .update_window(window, |_, window, _| !window.is_window_active())
        .unwrap();
    assert!(background, "test windows start inactive");

    for (share_details, body) in [
        (false, "Open Parabolica for details."),
        (true, "Can't play 曲.wav: missing"),
    ] {
        cx.update(|cx| {
            notify_failure(
                window,
                share_details,
                "Can't play 曲.wav: missing".into(),
                cx,
            )
        });
        let delivered = cx.delivered_system_notifications();
        // The same tag replaces the earlier notification.
        assert_eq!(delivered.len(), 1);
        assert_eq!(delivered[0].tag, FAILURE_TAG);
        assert_eq!(delivered[0].title, "Playback stopped");
        assert_eq!(delivered[0].body, body);
    }
}
