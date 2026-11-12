//! System media transport controls and the single-instance pipe.

use std::io;
use std::sync::mpsc;

use gpui_kit::Window;
use parabolica_platform::windows::lifecycle::{self, Claim};
use parabolica_platform::windows::media::{
    MediaButton, MediaDisplay, MediaSession, MediaState, MediaStatus as OsStatus,
};

use super::presentation::{MediaAction, MediaStatus, Presentation, apply};
use super::presenter::MediaSink;
use super::{APP_ID, APP_NAME, Instance, Launch, OpenRequest};
use crate::controllers::queue::QueueService;

/// Forwards to a running instance, or becomes the primary one. If the pipe cannot be used, the
/// app starts on its own; the database lock then reports a second instance.
pub(super) fn claim(startup: OpenRequest) -> Launch {
    match lifecycle::instance_name().and_then(|name| lifecycle::claim(&name)) {
        Ok(Claim::Running(running)) => match running.send(&startup.encode()) {
            Ok(()) => Launch::Forwarded,
            Err(_) => Launch::Primary(Instance::standalone(startup)),
        },
        Ok(Claim::Primary(server)) => {
            // Without the display name the overlay may name the app by its executable.
            let _ = lifecycle::register_app_identity(APP_ID, APP_NAME);
            let (sender, requests) = mpsc::channel();
            let server = server.serve(move |bytes| {
                OpenRequest::decode(&bytes).is_ok_and(|request| sender.send(request).is_ok())
            });
            Launch::Primary(Instance {
                startup,
                requests: Some(requests),
                _server: Some(server),
            })
        }
        Err(_) => Launch::Primary(Instance::standalone(startup)),
    }
}

pub(super) fn media_sink(window: &Window, queue: QueueService) -> io::Result<impl MediaSink> {
    let session = MediaSession::for_window(window, move |button| {
        let action = match button {
            MediaButton::Play => MediaAction::Play,
            MediaButton::Pause | MediaButton::Stop => MediaAction::Pause,
            MediaButton::Next => MediaAction::Next,
            MediaButton::Previous => MediaAction::Previous,
        };
        apply(action, &queue);
    })?;
    Ok(SmtcSink(session))
}

struct SmtcSink(MediaSession);

impl MediaSink for SmtcSink {
    fn show(&mut self, presentation: &Presentation, artwork: Option<&[u8]>) {
        let state = MediaState {
            status: match presentation.status {
                MediaStatus::Closed => OsStatus::Closed,
                MediaStatus::Stopped => OsStatus::Stopped,
                MediaStatus::Paused => OsStatus::Paused,
                MediaStatus::Playing => OsStatus::Playing,
            },
            display: presentation.details.as_ref().map(|details| MediaDisplay {
                title: details.title.clone(),
                artist: details.artist.clone(),
                album: details.album.clone(),
                artwork: artwork.map(<[u8]>::to_vec),
            }),
            can_play: presentation.can_play,
            can_pause: presentation.can_pause,
            can_next: presentation.can_next,
            can_previous: presentation.can_previous,
        };
        // Never leave an earlier entry's details behind after a failed update.
        if self.0.update(&state).is_err() {
            let _ = self.0.update(&MediaState {
                display: None,
                ..state
            });
        }
    }
}
