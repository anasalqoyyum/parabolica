//! Native ASIO output. The SDK owner runs on an STA; driver callbacks only render prepared PCM.

#[cfg(feature = "asio")]
mod stream;
#[cfg(feature = "asio")]
pub(super) use stream::{driver_panel, inspect, open};

#[cfg(any(feature = "asio", test))]
mod samples;
