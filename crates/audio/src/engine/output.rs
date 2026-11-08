//! The boundary between the engine and an output backend.

use parabolica_domain::format::SourceFormat;
use parabolica_domain::output::{NegotiatedOutput, OutputSelection};

use super::render::StreamRenderer;

/// Opens device streams. A backend such as WASAPI Shared implements it.
pub trait Output: Send + 'static {
    fn asio_capabilities(
        &mut self,
        _selection: &OutputSelection,
    ) -> Result<parabolica_domain::output::AsioCapabilities, OutputError> {
        Err(OutputError::BackendUnavailable)
    }

    fn driver_panel(&mut self, _selection: &OutputSelection) -> Result<(), OutputError> {
        Err(OutputError::BackendUnavailable)
    }

    /// Open a stream on the backend's device and negotiate its format without starting it.
    fn open(&mut self) -> Result<Box<dyn OutputStream>, OutputError>;

    fn open_for(
        &mut self,
        _selection: &OutputSelection,
        _source: &SourceFormat,
    ) -> Result<Box<dyn OutputStream>, OutputError> {
        self.open()
    }
}

/// An open device stream. Dropping it stops the stream and joins its device thread.
pub trait OutputStream: Send {
    /// The format and buffer the backend actually opened.
    fn negotiated(&self) -> &NegotiatedOutput;

    fn asio_capabilities(&self) -> Option<parabolica_domain::output::AsioCapabilities> {
        None
    }

    /// Start calling `renderer` from the backend's real-time thread.
    fn start(&mut self, renderer: StreamRenderer) -> Result<(), OutputError>;

    /// The error that stopped a started stream, such as the device being removed.
    fn failure(&self) -> Option<OutputError>;
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum OutputError {
    #[error("no audio output device is available")]
    NoDevice,
    #[error("the audio device was removed or changed")]
    DeviceLost,
    #[error("the requested output format is not supported: {0}")]
    UnsupportedFormat(String),
    #[error("the selected output is busy; close other audio apps and retry")]
    Busy,
    #[error("the selected output backend is not available in this build")]
    BackendUnavailable,
    #[error("the ASIO driver requested reconfiguration ({0}); retry the selected driver")]
    DriverReset(String),
    #[error("the audio device stopped responding")]
    Stalled,
    #[error("audio output failed: {0}")]
    Backend(String),
    #[error("audio output is not available on this platform yet")]
    Unavailable,
}

/// The output for platforms without a backend yet. Every open fails visibly.
#[derive(Debug, Default)]
pub struct NoOutput;

impl Output for NoOutput {
    fn open(&mut self) -> Result<Box<dyn OutputStream>, OutputError> {
        Err(OutputError::Unavailable)
    }
}
