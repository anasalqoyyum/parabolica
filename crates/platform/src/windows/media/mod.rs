//! System media transport controls (SMTC). Windows routes hardware media keys and the media
//! overlay's buttons to the session of the window passed to [`MediaSession::for_window`], also
//! while that window is unfocused.

#![allow(unsafe_code)]

use std::fmt;
use std::io;

use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use windows::Foundation::TypedEventHandler;
use windows::Media::{
    MediaPlaybackStatus, MediaPlaybackType, SystemMediaTransportControls,
    SystemMediaTransportControlsButton, SystemMediaTransportControlsButtonPressedEventArgs,
};
use windows::Storage::Streams::{
    DataWriter, InMemoryRandomAccessStream, RandomAccessStreamReference,
};
use windows::Win32::Foundation::HWND;
use windows::Win32::System::WinRT::ISystemMediaTransportControlsInterop;
use windows::core::{HSTRING, Interface as _, factory};
use windows_future::IAsyncOperation;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaButton {
    Play,
    Pause,
    Stop,
    Next,
    Previous,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaStatus {
    /// Nothing is selected. The overlay hides the session.
    Closed,
    Stopped,
    Paused,
    Playing,
}

/// What the overlay shows for the current entry. Every field is replaced on each update, so a
/// value from an earlier entry never remains.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct MediaDisplay {
    pub title: String,
    pub artist: String,
    pub album: String,
    /// Encoded PNG or JPEG.
    pub artwork: Option<Vec<u8>>,
}

impl fmt::Debug for MediaDisplay {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MediaDisplay")
            .field("title", &self.title)
            .field("artist", &self.artist)
            .field("album", &self.album)
            .field("artwork_bytes", &self.artwork.as_ref().map(Vec::len))
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaState {
    pub status: MediaStatus,
    /// `None` clears the overlay's text and artwork; the app name remains.
    pub display: Option<MediaDisplay>,
    pub can_play: bool,
    pub can_pause: bool,
    pub can_next: bool,
    pub can_previous: bool,
}

/// The window's SMTC session. Dropping it removes the button handler and clears the overlay.
pub struct MediaSession {
    controls: SystemMediaTransportControls,
    token: i64,
}

impl fmt::Debug for MediaSession {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MediaSession").finish_non_exhaustive()
    }
}

impl MediaSession {
    /// Binds to the top-level `window`. `on_button` runs on a system thread.
    pub fn for_window(
        window: &impl HasWindowHandle,
        on_button: impl Fn(MediaButton) + Send + Sync + 'static,
    ) -> io::Result<Self> {
        let handle = window
            .window_handle()
            .map_err(|error| io::Error::other(error.to_string()))?;
        let RawWindowHandle::Win32(handle) = handle.as_raw() else {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "the window has no Win32 handle",
            ));
        };
        let hwnd = HWND(handle.hwnd.get() as *mut _);
        let interop =
            factory::<SystemMediaTransportControls, ISystemMediaTransportControlsInterop>()
                .map_err(os_error)?;
        // SAFETY: `hwnd` is the live top-level window that `window` borrows.
        let controls: SystemMediaTransportControls =
            unsafe { interop.GetForWindow(hwnd) }.map_err(os_error)?;
        let handler = TypedEventHandler::<
            SystemMediaTransportControls,
            SystemMediaTransportControlsButtonPressedEventArgs,
        >::new(move |_, args| {
            let Some(args) = args.as_ref() else {
                return Ok(());
            };
            let button = match args.Button()? {
                SystemMediaTransportControlsButton::Play => MediaButton::Play,
                SystemMediaTransportControlsButton::Pause => MediaButton::Pause,
                SystemMediaTransportControlsButton::Stop => MediaButton::Stop,
                SystemMediaTransportControlsButton::Next => MediaButton::Next,
                SystemMediaTransportControlsButton::Previous => MediaButton::Previous,
                _ => return Ok(()),
            };
            on_button(button);
            Ok(())
        });
        let token = controls.ButtonPressed(&handler).map_err(os_error)?;
        let session = Self { controls, token };
        session.update(&MediaState {
            status: MediaStatus::Closed,
            display: None,
            can_play: false,
            can_pause: false,
            can_next: false,
            can_previous: false,
        })?;
        session.controls.SetIsEnabled(true).map_err(os_error)?;
        Ok(session)
    }

    pub fn update(&self, state: &MediaState) -> io::Result<()> {
        let controls = &self.controls;
        controls
            .SetPlaybackStatus(match state.status {
                MediaStatus::Closed => MediaPlaybackStatus::Closed,
                MediaStatus::Stopped => MediaPlaybackStatus::Stopped,
                MediaStatus::Paused => MediaPlaybackStatus::Paused,
                MediaStatus::Playing => MediaPlaybackStatus::Playing,
            })
            .map_err(os_error)?;
        controls
            .SetIsPlayEnabled(state.can_play)
            .map_err(os_error)?;
        controls
            .SetIsPauseEnabled(state.can_pause)
            .map_err(os_error)?;
        controls
            .SetIsStopEnabled(state.can_pause)
            .map_err(os_error)?;
        controls
            .SetIsNextEnabled(state.can_next)
            .map_err(os_error)?;
        controls
            .SetIsPreviousEnabled(state.can_previous)
            .map_err(os_error)?;

        let updater = controls.DisplayUpdater().map_err(os_error)?;
        updater.ClearAll().map_err(os_error)?;
        if let Some(display) = &state.display {
            updater
                .SetType(MediaPlaybackType::Music)
                .map_err(os_error)?;
            let music = updater.MusicProperties().map_err(os_error)?;
            music
                .SetTitle(&HSTRING::from(display.title.as_str()))
                .map_err(os_error)?;
            music
                .SetArtist(&HSTRING::from(display.artist.as_str()))
                .map_err(os_error)?;
            music
                .SetAlbumTitle(&HSTRING::from(display.album.as_str()))
                .map_err(os_error)?;
            if let Some(artwork) = &display.artwork {
                updater
                    .SetThumbnail(&artwork_stream(artwork).map_err(os_error)?)
                    .map_err(os_error)?;
            }
        }
        updater.Update().map_err(os_error)
    }
}

impl Drop for MediaSession {
    fn drop(&mut self) {
        let _ = self.controls.RemoveButtonPressed(self.token);
        if let Ok(updater) = self.controls.DisplayUpdater() {
            let _ = updater.ClearAll();
            let _ = updater.Update();
        }
        let _ = self.controls.SetPlaybackStatus(MediaPlaybackStatus::Closed);
        let _ = self.controls.SetIsEnabled(false);
    }
}

fn artwork_stream(bytes: &[u8]) -> windows::core::Result<RandomAccessStreamReference> {
    let stream = InMemoryRandomAccessStream::new()?;
    let writer = DataWriter::CreateDataWriter(&stream)?;
    writer.WriteBytes(bytes)?;
    let stored: IAsyncOperation<u32> = writer.StoreAsync()?.cast()?;
    stored.join()?;
    writer.FlushAsync()?.join()?;
    writer.DetachStream()?;
    stream.Seek(0)?;
    RandomAccessStreamReference::CreateFromStream(&stream)
}

fn os_error(error: windows::core::Error) -> io::Error {
    io::Error::other(format!("system media controls: {}", error.message()))
}
