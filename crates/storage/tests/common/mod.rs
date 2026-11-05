#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::time::Duration;

use parabolica_domain::files::{FileLocation, FileSignature};
use parabolica_domain::ids::{LibraryRootId, QueueEntryId};
use parabolica_domain::provider::{ProviderId, ProviderItemId, ProviderItemRef};
use parabolica_domain::queue::{QueueEntry, RepeatMode};
use parabolica_domain::source::PlayableSource;
use parabolica_storage::library::{DisplayTag, MediaFileRecord};
use parabolica_storage::preferences::Preference;
use parabolica_storage::queue::{QueueCheckpoint, Shuffle};

pub fn signature(size: u64) -> FileSignature {
    FileSignature {
        size_bytes: size,
        modified_unix_nanos: 1_700_000_000_000_000_000,
        content_digest: None,
    }
}

pub fn file_record(root: LibraryRootId, path: &str, tags: &[(&str, &str)]) -> MediaFileRecord {
    MediaFileRecord {
        location: FileLocation {
            root,
            relative_path: path.split('/').collect::<PathBuf>(),
        },
        os_file_id: None,
        signature: signature(1_000),
        tags: tags
            .iter()
            .map(|(field, value)| DisplayTag::new(*field, *value))
            .collect(),
    }
}

pub fn youtube(item: &str) -> ProviderItemRef {
    ProviderItemRef {
        provider: ProviderId::YouTubeMusic,
        item: ProviderItemId(item.to_owned()),
    }
}

/// A queue of `sources` with the second entry current and a reversed shuffle order.
pub fn queue_of(sources: Vec<PlayableSource>, position: Duration) -> QueueCheckpoint {
    let entries: Vec<QueueEntry> = sources
        .into_iter()
        .map(|source| QueueEntry {
            id: QueueEntryId::generate(),
            source,
        })
        .collect();
    QueueCheckpoint {
        current: entries.get(1).map(|entry| entry.id),
        shuffle: Some(Shuffle {
            seed: u64::MAX - 7,
            order: entries.iter().rev().map(|entry| entry.id).collect(),
        }),
        entries,
        position,
        repeat: RepeatMode::All,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppearanceMode {
    FollowSystem,
    Light,
    Dark,
}

impl Preference for AppearanceMode {
    const KEY: &'static str = "appearance.mode";

    fn encode(&self) -> String {
        match self {
            AppearanceMode::FollowSystem => "follow-system",
            AppearanceMode::Light => "light",
            AppearanceMode::Dark => "dark",
        }
        .to_owned()
    }

    fn decode(stored: &str) -> Option<Self> {
        match stored {
            "follow-system" => Some(AppearanceMode::FollowSystem),
            "light" => Some(AppearanceMode::Light),
            "dark" => Some(AppearanceMode::Dark),
            _ => None,
        }
    }
}

pub fn database_files(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}
