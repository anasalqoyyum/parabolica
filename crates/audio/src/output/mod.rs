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
