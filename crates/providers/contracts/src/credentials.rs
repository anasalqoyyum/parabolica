//! Where account credentials live: the operating system's secure store, never the database,
//! logs, exports or fixtures.

use std::fmt;

use crate::secret::Secret;
use crate::service::{AccountIdentity, ServiceId};

/// One saved sign-in: the account it belongs to and its secret.
#[derive(Debug, Clone)]
pub struct StoredCredential {
    pub account: AccountIdentity,
    pub secret: Secret,
}

/// Credential store failures. Messages name the operation and OS error code, never a value.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CredentialError {
    #[error("no secure credential store is available on this system")]
    Unavailable,
    #[error("the {field} is too large for the system credential store ({len}, at most {max})")]
    TooLarge {
        field: &'static str,
        len: usize,
        max: usize,
    },
    #[error("the system credential store could not {operation} the sign-in (error {code:#010x})")]
    Os { operation: &'static str, code: u32 },
    #[error("the saved sign-in could not be read")]
    Unreadable,
}

/// The OS-backed store for one credential per service. Calls block; run them off the UI thread.
pub trait CredentialStore: Send + Sync + fmt::Debug {
    fn load(&self, service: ServiceId) -> Result<Option<StoredCredential>, CredentialError>;

    /// Replaces any credential saved for `service`.
    fn save(
        &self,
        service: ServiceId,
        credential: &StoredCredential,
    ) -> Result<(), CredentialError>;

    /// Removes the credential for `service`. Removing an absent credential succeeds.
    fn delete(&self, service: ServiceId) -> Result<(), CredentialError>;
}

#[cfg(any(test, feature = "test-support"))]
pub use memory::MemoryCredentialStore;

#[cfg(any(test, feature = "test-support"))]
mod memory {
    use std::collections::BTreeMap;
    use std::sync::Mutex;

    use super::{CredentialError, CredentialStore, StoredCredential};
    use crate::service::ServiceId;

    /// A process-local store for tests. Set [`MemoryCredentialStore::fail_with`] to make every
    /// call fail.
    #[derive(Debug, Default)]
    pub struct MemoryCredentialStore {
        state: Mutex<State>,
    }

    #[derive(Debug, Default)]
    struct State {
        saved: BTreeMap<ServiceId, StoredCredential>,
        failure: Option<CredentialError>,
    }

    impl MemoryCredentialStore {
        pub fn new() -> Self {
            Self::default()
        }

        pub fn fail_with(&self, failure: Option<CredentialError>) {
            self.state.lock().unwrap().failure = failure;
        }

        pub fn contains(&self, service: ServiceId) -> bool {
            self.state.lock().unwrap().saved.contains_key(&service)
        }
    }

    impl CredentialStore for MemoryCredentialStore {
        fn load(&self, service: ServiceId) -> Result<Option<StoredCredential>, CredentialError> {
            let state = self.state.lock().unwrap();
            match &state.failure {
                Some(failure) => Err(failure.clone()),
                None => Ok(state.saved.get(&service).cloned()),
            }
        }

        fn save(
            &self,
            service: ServiceId,
            credential: &StoredCredential,
        ) -> Result<(), CredentialError> {
            let mut state = self.state.lock().unwrap();
            if let Some(failure) = &state.failure {
                return Err(failure.clone());
            }
            state.saved.insert(service, credential.clone());
            Ok(())
        }

        fn delete(&self, service: ServiceId) -> Result<(), CredentialError> {
            let mut state = self.state.lock().unwrap();
            if let Some(failure) = &state.failure {
                return Err(failure.clone());
            }
            state.saved.remove(&service);
            Ok(())
        }
    }
}
