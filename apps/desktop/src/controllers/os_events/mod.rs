//! OS media keys and overlay, open-with and second-instance requests, playback notifications,
//! sleep/resume and quit.
//!
//! Media buttons and open requests use the same queue commands as the player. Sleep pauses and
//! waits for the checkpoint; wake refreshes devices but never resumes or reroutes. Quit joins the
//! queue worker's final checkpoint before GPUI's shutdown timeout can cut it short.

#[cfg_attr(
    not(any(windows, test)),
    expect(
        dead_code,
        reason = "only Windows forwards requests to a running instance"
    )
)]
mod open_request;
#[cfg_attr(
    not(any(windows, test)),
    expect(dead_code, reason = "only Windows has an OS media controls backend")
)]
mod presentation;
#[cfg_attr(
    not(any(windows, test)),
    expect(dead_code, reason = "only Windows has an OS media controls backend")
)]
mod presenter;
#[cfg(test)]
mod tests;
#[cfg(windows)]
mod windows;

use std::path::Path;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use gpui_kit::{
    AnyWindowHandle, App, Entity, Global, SharedString, Subscription, SystemNotification, Task,
    Window,
};
use parabolica_domain::ids::QueueEntryId;
use parabolica_domain::playback::TransportState;
use parabolica_storage::Storage;
use parabolica_storage::preferences::Preference;

pub use open_request::OpenRequest;
use presentation::{Presentation, present};
use presenter::Presenter;

use crate::controllers::queue::{EnqueueMode, QueueService};
use crate::ui::player::Player;
use crate::ui::shell::Shell;

/// The AppUserModelID that names the app in the media overlay and notifications.
pub const APP_ID: &str = "Parabolica.Desktop";
const APP_NAME: &str = "Parabolica";
const REQUEST_POLL: Duration = Duration::from_millis(100);
/// Windows allows a short time to handle a suspend notification.
const SLEEP_CHECKPOINT_WAIT: Duration = Duration::from_secs(1);
const FAILURE_TAG: &str = "playback-stopped";

pub enum Launch {
    Primary(Instance),
    /// A running instance accepted this launch's files. Exit without opening a window.
    #[cfg_attr(
        not(windows),
        expect(
            dead_code,
            reason = "only Windows forwards requests to a running instance"
        )
    )]
    Forwarded,
}

/// This process as the primary instance: its own command-line files and, on Windows, the
/// requests later launches send.
pub struct Instance {
    startup: OpenRequest,
    requests: Option<mpsc::Receiver<OpenRequest>>,
    #[cfg(windows)]
    _server: Option<parabolica_platform::windows::lifecycle::ServerHandle>,
}

impl Instance {
    fn standalone(startup: OpenRequest) -> Self {
        Self {
            startup,
            requests: None,
            #[cfg(windows)]
            _server: None,
        }
    }
}

/// Runs first in `main`, before storage opens: a second launch hands its files to the running
/// player instead of failing on the database lock.
pub fn claim_instance() -> Launch {
    let working_dir = std::env::current_dir().ok();
    let startup = OpenRequest::from_args(std::env::args_os().skip(1), working_dir.as_deref());
    #[cfg(windows)]
    {
        windows::claim(startup)
    }
    #[cfg(not(windows))]
    {
        Launch::Primary(Instance::standalone(startup))
    }
}

/// Whether the OS media controls and notifications may show the track's title, artist, album
/// and artwork. On by default; off still offers the playback buttons.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShareTrackDetails(pub bool);

impl Preference for ShareTrackDetails {
    const KEY: &'static str = "system_media.track_details";

    fn encode(&self) -> String {
        if self.0 { "shown" } else { "hidden" }.to_owned()
    }

    fn decode(stored: &str) -> Option<Self> {
        match stored {
            "shown" => Some(Self(true)),
            "hidden" => Some(Self(false)),
            _ => None,
        }
    }
}

/// Read before the event loop, like the appearance, so the first presentation already respects
/// the saved choice. An unreadable value hides details.
pub fn load_saved_sharing(storage: Option<&Storage>) -> bool {
    match storage.map(Storage::preference::<ShareTrackDetails>) {
        Some(Ok(saved)) => saved.is_none_or(|ShareTrackDetails(shown)| shown),
        Some(Err(_)) => false,
        None => true,
    }
}

struct OsEvents {
    instance: Instance,
    storage: Option<Storage>,
    share_details: bool,
    persisted: Option<bool>,
    saving: bool,
    save_error: Option<SharedString>,
    attached: Option<Attached>,
    _subscriptions: [Subscription; 3],
}

impl Global for OsEvents {}

struct Attached {
    window: AnyWindowHandle,
    player: Entity<Player>,
    queue: QueueService,
    presenter: Option<Presenter>,
    shown: Option<Presentation>,
    failure: Option<(Option<QueueEntryId>, String)>,
    _observe: Subscription,
    _poll: Task<()>,
}

/// Registers lifecycle handlers. Call before the main window opens.
pub fn init(instance: Instance, share_details: bool, storage: Option<Storage>, cx: &mut App) {
    cx.set_app_identity(APP_ID, APP_NAME);
    cx.on_system_notification_response(|_, cx| activate(cx));
    let subscriptions = [
        cx.on_system_sleep(before_sleep),
        cx.on_system_wake(after_wake),
        cx.on_app_quit(|cx| {
            shutdown(cx);
            async {}
        }),
    ];
    cx.set_global(OsEvents {
        instance,
        storage,
        share_details,
        persisted: Some(share_details),
        saving: false,
        save_error: None,
        attached: None,
        _subscriptions: subscriptions,
    });
}

/// Connects the main window's player: OS media controls, open requests and notifications.
pub fn attach(window: AnyWindowHandle, shell: &Entity<Shell>, cx: &mut App) {
    let Ok((player, presenter)) = window.update(cx, |_, window, cx| {
        let player = shell.read(cx).player().clone();
        let presenter = media_presenter(window, player.read(cx).queue());
        (player, presenter)
    }) else {
        return;
    };
    let queue = player.read(cx).queue();
    let observe = cx.observe(&player, |_, cx| refresh(cx));
    let poll = cx.spawn(async move |cx| {
        loop {
            cx.background_executor().timer(REQUEST_POLL).await;
            if !cx.update(drain_requests) {
                break;
            }
        }
    });
    let state = cx.global_mut::<OsEvents>();
    let startup = std::mem::take(&mut state.instance.startup);
    state.attached = Some(Attached {
        window,
        player,
        queue,
        presenter,
        shown: None,
        failure: None,
        _observe: observe,
        _poll: poll,
    });
    open(startup, false, cx);
    refresh(cx);
}

pub fn share_details(cx: &App) -> bool {
    cx.try_global::<OsEvents>()
        .is_none_or(|state| state.share_details)
}

pub fn save_error(cx: &App) -> Option<SharedString> {
    cx.try_global::<OsEvents>()?.save_error.clone()
}

pub fn set_share_details(shown: bool, cx: &mut App) {
    if !cx.has_global::<OsEvents>() {
        return;
    }
    cx.global_mut::<OsEvents>().share_details = shown;
    refresh(cx);
    persist(cx);
    cx.refresh_windows();
}

#[cfg(windows)]
fn media_presenter(window: &Window, queue: QueueService) -> Option<Presenter> {
    windows::media_sink(window, queue)
        .ok()
        .map(|sink| Presenter::start(sink, artwork_loader()))
}

#[cfg(not(windows))]
fn media_presenter(_: &Window, _: QueueService) -> Option<Presenter> {
    None
}

/// Embedded or folder artwork as a 256 px PNG, through the library's bounded loader.
#[cfg(windows)]
fn artwork_loader() -> presenter::ArtworkLoader {
    use parabolica_domain::control::CancelToken;
    use presentation::Artwork;

    let mut cache = parabolica_library::artwork::ThumbnailCache::default();
    Box::new(move |artwork| {
        let Artwork::LocalFile { file, path } = artwork else {
            return None;
        };
        let signature = std::fs::metadata(path)
            .and_then(|metadata| parabolica_library::index::signature(&metadata))
            .ok()?;
        cache
            .load(*file, signature, path, &CancelToken::new())
            .ok()
            .flatten()
    })
}

/// Recomputes what the OS shows after any player or queue change.
fn refresh(cx: &mut App) {
    let Some(state) = cx.try_global::<OsEvents>() else {
        return;
    };
    let Some(attached) = &state.attached else {
        return;
    };
    let snapshot = attached.queue.snapshot();
    let engine = attached.player.read(cx).output_snapshot();
    let presentation = present(&snapshot, engine, state.share_details);
    let failure = engine
        .and_then(|engine| engine.failure.as_ref())
        .map(|failure| {
            let file = failure.path.as_deref().and_then(Path::file_name);
            let text = match file {
                Some(file) => format!("Can't play {}: {}", file.to_string_lossy(), failure.error),
                None => format!("Can't play: {}", failure.error),
            };
            ((failure.entry, failure.error.to_string()), text)
        });
    let share_details = state.share_details;
    let window = attached.window;

    let attached = cx
        .global_mut::<OsEvents>()
        .attached
        .as_mut()
        .expect("checked above");
    if attached.shown.as_ref() != Some(&presentation) {
        if let Some(presenter) = &attached.presenter {
            presenter.present(presentation.clone());
        }
        attached.shown = Some(presentation);
    }
    let failure_key = failure.as_ref().map(|(key, _)| key.clone());
    if attached.failure == failure_key {
        return;
    }
    attached.failure = failure_key;
    if let Some((_, text)) = failure {
        notify_failure(window, share_details, text, cx);
    }
}

/// Asks for attention only while the window is in the background; the player shows the error.
fn notify_failure(window: AnyWindowHandle, share_details: bool, text: String, cx: &mut App) {
    let background = window
        .update(cx, |_, window, _| {
            let background = !window.is_window_active();
            if background {
                window.request_attention();
            }
            background
        })
        .unwrap_or(false);
    if background {
        cx.show_system_notification(SystemNotification {
            tag: FAILURE_TAG.into(),
            title: "Playback stopped".into(),
            body: if share_details {
                text
            } else {
                "Open Parabolica for details.".to_owned()
            }
            .into(),
            actions: Vec::new(),
        });
    }
}

/// Returns whether the window is still attached.
fn drain_requests(cx: &mut App) -> bool {
    let Some(state) = cx.try_global::<OsEvents>() else {
        return false;
    };
    if state.attached.is_none() {
        return false;
    }
    let requests: Vec<_> = state
        .instance
        .requests
        .as_ref()
        .map(|requests| requests.try_iter().collect())
        .unwrap_or_default();
    for request in requests {
        open(request, true, cx);
    }
    true
}

/// Plays the request's files as the player's Open file does, then shows the window.
fn open(request: OpenRequest, show_window: bool, cx: &mut App) {
    let Some(attached) = &cx.global::<OsEvents>().attached else {
        return;
    };
    if !request.paths.is_empty() {
        attached
            .queue
            .enqueue_files(request.paths, EnqueueMode::Now);
    }
    if show_window {
        activate(cx);
    }
}

fn activate(cx: &mut App) {
    if let Some(attached) = cx
        .try_global::<OsEvents>()
        .and_then(|state| state.attached.as_ref())
    {
        let _ = attached
            .window
            .update(cx, |_, window, _| window.activate_window());
    }
}

fn attached_queue(cx: &App) -> Option<QueueService> {
    Some(
        cx.try_global::<OsEvents>()?
            .attached
            .as_ref()?
            .queue
            .clone(),
    )
}

/// Pauses and waits for the paused checkpoint, so nothing plays when the system wakes.
fn before_sleep(cx: &mut App) {
    if let Some(queue) = attached_queue(cx) {
        pause_for_sleep(&queue, SLEEP_CHECKPOINT_WAIT);
    }
}

fn pause_for_sleep(queue: &QueueService, wait: Duration) {
    let playing = |queue: &QueueService| {
        matches!(
            queue.snapshot().transport,
            TransportState::Playing | TransportState::Loading
        )
    };
    if !playing(queue) {
        return;
    }
    queue.pause();
    let deadline = Instant::now() + wait;
    while playing(queue) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// Devices may have changed while asleep. Playback stays paused on the saved endpoint until the
/// user presses Play.
fn after_wake(cx: &mut App) {
    if let Some(queue) = attached_queue(cx) {
        queue.refresh_outputs();
    }
}

/// Blocks the UI thread until the queue worker's final checkpoint is saved, then clears the OS
/// media controls. Runs in the quit handler's body because GPUI awaits returned futures for only
/// 200 ms, and on Windows session end the process exits right after.
fn shutdown(cx: &mut App) {
    if !cx.has_global::<OsEvents>() {
        return;
    }
    let state = cx.global_mut::<OsEvents>();
    #[cfg(windows)]
    {
        state.instance._server = None;
    }
    if let Some(attached) = state.attached.take() {
        attached.queue.shutdown();
    }
}

/// Saves the choice off the UI thread, one write at a time, so the last choice is stored.
fn persist(cx: &mut App) {
    let state = cx.global_mut::<OsEvents>();
    if state.saving || state.persisted == Some(state.share_details) {
        return;
    }
    let Some(storage) = state.storage.clone() else {
        return;
    };
    let shown = state.share_details;
    state.saving = true;
    let save = cx
        .background_executor()
        .spawn(async move { storage.save_preference(&ShareTrackDetails(shown)) });
    cx.spawn(async move |cx| {
        let result = save.await;
        cx.update(|cx| {
            let state = cx.global_mut::<OsEvents>();
            state.saving = false;
            match result {
                Ok(()) => {
                    state.persisted = Some(shown);
                    state.save_error = None;
                }
                Err(error) => {
                    state.save_error = Some(format!("Setting not saved: {error}").into());
                }
            }
            let newer = state.share_details != shown;
            cx.refresh_windows();
            if newer {
                persist(cx);
            }
        });
    })
    .detach();
}
