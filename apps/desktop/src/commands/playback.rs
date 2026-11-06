//! Dispatches playback commands from the UI to the playback controller.

use std::path::PathBuf;
use std::sync::mpsc::Receiver;
use std::time::Duration;

use parabolica_audio::engine::{EngineEvent, Output, PlaybackEngine};
use parabolica_domain::gain::Volume;
use parabolica_domain::ids::{MediaFileId, QueueEntryId};
use parabolica_domain::source::ResolvedSource;

/// The UI's handle to the playback engine. Every method returns without waiting.
#[derive(Debug)]
pub struct PlaybackCommands {
    engine: PlaybackEngine,
}

impl PlaybackCommands {
    /// Start the engine on this platform's default output.
    pub fn start() -> (Self, Receiver<EngineEvent>) {
        Self::with_output(default_output())
    }

    pub fn with_output(output: Box<dyn Output>) -> (Self, Receiver<EngineEvent>) {
        let (engine, events) = PlaybackEngine::start(output);
        (Self { engine }, events)
    }

    /// Play a file chosen outside the library. It gets a fresh queue entry and file identity until
    /// library indexing supplies stable ones.
    pub fn open_file(&self, path: PathBuf) -> QueueEntryId {
        let entry = QueueEntryId::generate();
        self.engine.play(
            entry,
            ResolvedSource::local_file(MediaFileId::generate(), path),
        );
        entry
    }

    pub fn pause(&self) {
        self.engine.pause();
    }

    pub fn resume(&self) {
        self.engine.resume();
    }

    pub fn seek(&self, entry: QueueEntryId, position: Duration) {
        self.engine.seek(entry, position);
    }

    pub fn set_volume(&self, volume: Volume) {
        self.engine.set_volume(volume);
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
