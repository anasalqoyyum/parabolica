//! OS credential storage and the system browser for sign-in. Secrets never reach the database,
//! logs or exports.
//!
//! On Windows, each service's sign-in is one generic credential in Credential Manager, named
//! `Parabolica/<service key>` and persisted for the current user on this computer
//! (`CRED_PERSIST_LOCAL_MACHINE`, not roaming). The account ID is the credential's user name,
//! the display name its comment and the secret its blob. macOS Keychain and Linux Secret Service
//! stores belong to the platform ports; until then those builds report
//! [`CredentialError::Unavailable`].

#[cfg(windows)]
mod credential_manager;
#[cfg(all(test, windows))]
mod tests;

use parabolica_provider_contracts::auth::{AuthorizationError, Browser};
use parabolica_provider_contracts::credentials::{
    CredentialError, CredentialStore, StoredCredential,
};
use parabolica_provider_contracts::service::ServiceId;
use url::Url;

#[cfg(windows)]
use credential_manager as imp;

/// The operating system's credential store.
#[derive(Debug, Clone)]
pub struct OsCredentialStore {
    #[cfg_attr(
        not(windows),
        expect(dead_code, reason = "no store on this platform yet")
    )]
    namespace: String,
}

impl Default for OsCredentialStore {
    fn default() -> Self {
        Self::new()
    }
}

impl OsCredentialStore {
    pub fn new() -> Self {
        Self::with_namespace("Parabolica")
    }

    /// Credentials named `<namespace>/<service key>`, so tests never touch the app's own.
    fn with_namespace(namespace: &str) -> Self {
        Self {
            namespace: namespace.to_owned(),
        }
    }

    #[cfg_attr(
        not(windows),
        expect(dead_code, reason = "no store on this platform yet")
    )]
    fn target(&self, service: ServiceId) -> String {
        format!("{}/{}", self.namespace, service.key())
    }
}

#[cfg(windows)]
impl CredentialStore for OsCredentialStore {
    fn load(&self, service: ServiceId) -> Result<Option<StoredCredential>, CredentialError> {
        imp::read(&self.target(service))
    }

    fn save(
        &self,
        service: ServiceId,
        credential: &StoredCredential,
    ) -> Result<(), CredentialError> {
        imp::write(&self.target(service), credential)
    }

    fn delete(&self, service: ServiceId) -> Result<(), CredentialError> {
        imp::delete(&self.target(service))
    }
}

#[cfg(not(windows))]
impl CredentialStore for OsCredentialStore {
    fn load(&self, _: ServiceId) -> Result<Option<StoredCredential>, CredentialError> {
        Err(CredentialError::Unavailable)
    }

    fn save(&self, _: ServiceId, _: &StoredCredential) -> Result<(), CredentialError> {
        Err(CredentialError::Unavailable)
    }

    fn delete(&self, _: ServiceId) -> Result<(), CredentialError> {
        Err(CredentialError::Unavailable)
    }
}

/// The user's default browser. Only `https` and `http` URLs are opened.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemBrowser;

impl Browser for SystemBrowser {
    fn open(&self, url: &Url) -> Result<(), AuthorizationError> {
        if !matches!(url.scheme(), "https" | "http") {
            return Err(AuthorizationError::Browser);
        }
        open_url(url.as_str())
    }
}

#[cfg(windows)]
fn open_url(url: &str) -> Result<(), AuthorizationError> {
    imp::open_url(url)
}

#[cfg(not(windows))]
fn open_url(url: &str) -> Result<(), AuthorizationError> {
    let opener = if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    match std::process::Command::new(opener).arg(url).status() {
        Ok(status) if status.success() => Ok(()),
        _ => Err(AuthorizationError::Browser),
    }
}
