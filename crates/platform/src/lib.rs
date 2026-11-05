//! Operating-system integration kept out of the UI and domain crates.

pub mod contrast;
pub mod credentials;

#[cfg(target_os = "linux")]
pub mod linux;
#[cfg(target_os = "macos")]
pub mod macos;
#[cfg(windows)]
pub mod windows;
