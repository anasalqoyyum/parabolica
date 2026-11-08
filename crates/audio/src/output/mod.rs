//! Output backends.

#[cfg(windows)]
pub mod asio;
#[cfg(target_os = "macos")]
pub mod coreaudio;
#[cfg(target_os = "linux")]
pub mod linux;
#[cfg(windows)]
pub mod wasapi_exclusive;
#[cfg(windows)]
pub mod wasapi_shared;

#[cfg(windows)]
#[derive(Debug, Default)]
pub struct WindowsOutput;

#[cfg(windows)]
impl crate::engine::Output for WindowsOutput {
    fn open(&mut self) -> Result<Box<dyn crate::engine::OutputStream>, crate::engine::OutputError> {
        wasapi_shared::WasapiOutput.open()
    }

    fn open_for(
        &mut self,
        selection: &parabolica_domain::output::OutputSelection,
        source: &parabolica_domain::format::SourceFormat,
    ) -> Result<Box<dyn crate::engine::OutputStream>, crate::engine::OutputError> {
        match selection.backend {
            parabolica_domain::output::OutputBackend::WasapiShared
            | parabolica_domain::output::OutputBackend::WasapiExclusive => {
                wasapi_shared::WasapiOutput.open_for(selection, source)
            }
            #[cfg(feature = "asio")]
            parabolica_domain::output::OutputBackend::Asio => asio::open(selection, source),
            _ => Err(crate::engine::OutputError::BackendUnavailable),
        }
    }

    fn asio_capabilities(
        &mut self,
        selection: &parabolica_domain::output::OutputSelection,
    ) -> Result<parabolica_domain::output::AsioCapabilities, crate::engine::OutputError> {
        #[cfg(feature = "asio")]
        {
            asio::inspect(selection)
        }
        #[cfg(not(feature = "asio"))]
        {
            let _ = selection;
            Err(crate::engine::OutputError::BackendUnavailable)
        }
    }

    fn driver_panel(
        &mut self,
        selection: &parabolica_domain::output::OutputSelection,
    ) -> Result<(), crate::engine::OutputError> {
        #[cfg(feature = "asio")]
        {
            asio::driver_panel(selection)
        }
        #[cfg(not(feature = "asio"))]
        {
            let _ = selection;
            Err(crate::engine::OutputError::BackendUnavailable)
        }
    }
}
