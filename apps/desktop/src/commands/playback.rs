//! Playback and queue commands. UI methods enqueue work and read memory-only snapshots.

use std::path::PathBuf;
use std::sync::{Arc, mpsc::Receiver};
use std::time::Duration;

use parabolica_audio::engine::{EngineEvent, Output};
use parabolica_domain::gain::Volume;
use parabolica_domain::ids::QueueEntryId;
use parabolica_storage::Storage;

pub use crate::controllers::queue::{EnqueueMode, QueueService, QueueSnapshot};

#[derive(Debug, Clone)]
pub struct PlaybackCommands {
    queue: QueueService,
}

impl PlaybackCommands {
    pub fn start_with_storage(storage: Option<Storage>) -> (Self, Receiver<EngineEvent>) {
        Self::with_storage_and_output(storage, default_output())
    }
    #[cfg(test)]
    pub fn with_output(output: Box<dyn Output>) -> (Self, Receiver<EngineEvent>) {
        Self::with_storage_and_output(None, output)
    }
    pub fn with_storage_and_output(
        storage: Option<Storage>,
        output: Box<dyn Output>,
    ) -> (Self, Receiver<EngineEvent>) {
        let (queue, events) = QueueService::start(storage, output);
        (Self { queue }, events)
    }
    pub fn queue(&self) -> &QueueService {
        &self.queue
    }
    pub fn snapshot(&self) -> Arc<QueueSnapshot> {
        self.queue.snapshot()
    }
    pub fn open_file(&self, path: PathBuf) -> QueueEntryId {
        self.queue.enqueue_files(vec![path], EnqueueMode::Now)[0]
    }
    pub fn pause(&self) {
        self.queue.pause();
    }
    pub fn resume(&self) {
        self.queue.resume();
    }
    pub fn seek(&self, entry: QueueEntryId, position: Duration) {
        self.queue.seek(entry, position);
    }
    pub fn set_volume(&self, volume: Volume) {
        self.queue.set_volume(volume);
    }
    pub fn next(&self) {
        self.queue.next();
    }
    pub fn previous(&self) {
        self.queue.previous();
    }
}

#[cfg(windows)]
fn default_output() -> Box<dyn Output> {
    Box::new(parabolica_audio::output::wasapi_shared::WasapiShared)
}
#[cfg(not(windows))]
fn default_output() -> Box<dyn Output> {
    Box::new(parabolica_audio::engine::NoOutput)
}
