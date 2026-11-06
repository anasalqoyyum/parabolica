//! Provider adapter traits and credential storage contracts.
//!
//! - [`service`]: services an account connects to and the account identity.
//! - [`secret`]: the redacted container for authentication material.
//! - [`credentials`]: the OS-backed [`credentials::CredentialStore`] contract.
//! - [`capabilities`]: what a provider can do for the current account and network state.
//! - [`requests`]: cancellable, generation-tagged requests that end with the account session.
//! - [`auth`]: browser sign-in flows, state/PKCE validation and the loopback callback receiver.
//! - [`account`]: the per-service account lifecycle, its status snapshots and events.
//!
//! See `docs/ARCHITECTURE.md` › Provider accounts.

pub mod account;
pub mod auth;
pub mod capabilities;
pub mod credentials;
pub mod error;
pub mod requests;
pub mod secret;
pub mod service;
