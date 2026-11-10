//! What the OS media controls show, derived from the queue and engine snapshots.

use std::path::PathBuf;

use parabolica_audio::engine::EngineSnapshot;
use parabolica_domain::ids::{MediaFileId, QueueEntryId};
use parabolica_domain::playback::TransportState;
use parabolica_domain::source::PlayableSource;

use crate::controllers::queue::{QueueService, QueueSnapshot};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaStatus {
    /// Nothing is selected.
    Closed,
    Stopped,
    Paused,
    Playing,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Artwork {
    None,
    LocalFile { file: MediaFileId, path: PathBuf },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Details {
    pub entry: QueueEntryId,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub artwork: Artwork,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Presentation {
    pub status: MediaStatus,
    /// `None` when nothing is selected or the user turned track details off.
    pub details: Option<Details>,
    pub can_play: bool,
    pub can_pause: bool,
    pub can_next: bool,
    pub can_previous: bool,
}

impl Presentation {
    pub const CLOSED: Self = Self {
        status: MediaStatus::Closed,
        details: None,
        can_play: false,
        can_pause: false,
        can_next: false,
        can_previous: false,
    };

    pub fn artwork(&self) -> &Artwork {
        self.details
            .as_ref()
            .map_or(&Artwork::None, |details| &details.artwork)
    }
}

/// The OS buttons. Stop pauses, as the player has no separate stop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaAction {
    Play,
    Pause,
    Next,
    Previous,
}

/// Sends the same queue command the player's button sends.
pub fn apply(action: MediaAction, queue: &QueueService) {
    match action {
        MediaAction::Play => queue.resume(),
        MediaAction::Pause => queue.pause(),
        MediaAction::Next => queue.next(),
        MediaAction::Previous => queue.previous(),
    };
}

/// Details come from the queue's current entry. Decoder tags are used only while the engine
/// reports that same entry, so a previous track's title, artist or artwork never shows for a
/// newly selected one.
pub fn present(
    queue: &QueueSnapshot,
    engine: Option<&EngineSnapshot>,
    share_details: bool,
) -> Presentation {
    let Some(current) = queue
        .current
        .and_then(|current| queue.entries.iter().find(|entry| entry.entry.id == current))
    else {
        // Play starts the first entry, as the player's button does.
        let queued = !queue.entries.is_empty();
        return Presentation {
            status: if queued {
                MediaStatus::Stopped
            } else {
                MediaStatus::Closed
            },
            can_play: queued,
            can_next: queued,
            can_previous: queued,
            ..Presentation::CLOSED
        };
    };
    let playing = matches!(
        queue.transport,
        TransportState::Playing | TransportState::Loading
    );
    let status = match queue.transport {
        TransportState::Playing | TransportState::Loading => MediaStatus::Playing,
        TransportState::Paused => MediaStatus::Paused,
        TransportState::Stopped | TransportState::Blocked => MediaStatus::Stopped,
    };
    let details = share_details.then(|| {
        let tags = engine
            .filter(|engine| engine.playback.current == Some(current.entry.id))
            .and_then(|engine| engine.source.as_ref())
            .map(|source| &source.tags);
        let artwork = match (&current.entry.source, &current.details.path) {
            (PlayableSource::LocalFile(file), Some(path)) => Artwork::LocalFile {
                file: *file,
                path: path.clone(),
            },
            _ => Artwork::None,
        };
        Details {
            entry: current.entry.id,
            title: tags
                .and_then(|tags| tags.title.clone())
                .unwrap_or_else(|| current.details.title.clone()),
            artist: tags
                .and_then(|tags| tags.artist.clone())
                .unwrap_or_default(),
            album: tags.and_then(|tags| tags.album.clone()).unwrap_or_default(),
            artwork,
        }
    });
    Presentation {
        status,
        details,
        can_play: !playing,
        can_pause: playing,
        can_next: true,
        can_previous: true,
    }
}
