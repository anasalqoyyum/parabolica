//! Browser sign-in.
//!
//! Providers sign in through the user's default browser, never through a password field in the
//! app. A provider implements [`AuthFlow`]; [`crate::account::Account`] runs it, rejects stale or
//! mismatched results and saves the credential. [`BrowserAuthorization`] and
//! [`loopback::LoopbackReceiver`] implement the state and PKCE checks of an OAuth-style browser
//! redirect for flows that use one.

pub mod loopback;

use std::borrow::Cow;
use std::fmt;
use std::io;

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use parabolica_domain::control::{CancelToken, Generation};
use sha2::{Digest as _, Sha256};
pub use url::Url;

use crate::credentials::StoredCredential;
use crate::error::{ProviderError, ProviderErrorKind};
use crate::secret::{Secret, constant_time_eq};
use crate::service::{AccountIdentity, ServiceId};

/// Sign-in failures. Messages never echo what the browser or service sent.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AuthorizationError {
    #[error("sign-in was declined in the browser")]
    Denied,
    #[error("the sign-in response did not match this request")]
    StateMismatch,
    #[error("the sign-in response had no authorization code")]
    MissingCode,
    #[error("sign-in timed out")]
    TimedOut,
    #[error("signed in to a different account than the one being reconnected")]
    WrongAccount,
    #[error("the browser could not be opened")]
    Browser,
    #[error("the sign-in response could not be received ({0})")]
    Callback(io::ErrorKind),
    #[error("the system random number generator failed")]
    Random,
}

/// One sign-in attempt, issued by [`crate::account::Account::begin_sign_in`].
#[derive(Debug, Clone)]
pub struct AuthAttempt {
    service: ServiceId,
    generation: Generation,
    reconnecting: Option<AccountIdentity>,
    cancel: CancelToken,
}

impl AuthAttempt {
    pub(crate) fn new(
        service: ServiceId,
        generation: Generation,
        reconnecting: Option<AccountIdentity>,
        cancel: CancelToken,
    ) -> Self {
        Self {
            service,
            generation,
            reconnecting,
            cancel,
        }
    }

    pub fn service(&self) -> ServiceId {
        self.service
    }

    pub(crate) fn generation(&self) -> Generation {
        self.generation
    }

    /// The account being reconnected. A sign-in to any other account is rejected.
    pub fn reconnecting(&self) -> Option<&AccountIdentity> {
        self.reconnecting.as_ref()
    }

    /// Cancelled by the user, by a newer attempt or by signing out.
    pub fn is_cancelled(&self) -> bool {
        self.cancel.is_cancelled()
    }

    pub fn check(&self) -> Result<(), ProviderError> {
        if self.is_cancelled() {
            Err(ProviderError::new(
                self.service,
                ProviderErrorKind::Cancelled,
            ))
        } else {
            Ok(())
        }
    }

    pub fn fail(&self, error: impl Into<ProviderErrorKind>) -> ProviderError {
        ProviderError::new(self.service, error)
    }
}

/// The result of a successful sign-in, before it is accepted and saved.
#[derive(Debug, Clone)]
pub struct SignedIn {
    pub account: AccountIdentity,
    pub secret: Secret,
}

/// Opens URLs in the user's default browser.
pub trait Browser: Send + Sync + fmt::Debug {
    fn open(&self, url: &Url) -> Result<(), AuthorizationError>;
}

/// A provider's sign-in and revocation. Implementations must not ship a shared personal token or
/// collect a password in native UI: every credential comes from the user's own sign-in.
pub trait AuthFlow: Send + Sync + fmt::Debug {
    /// Signs in through `browser`, blocking until the service answers, the attempt is cancelled
    /// or the flow times out. Poll [`AuthAttempt::check`] while waiting.
    fn sign_in(
        &self,
        attempt: &AuthAttempt,
        browser: &dyn Browser,
    ) -> Result<SignedIn, ProviderError>;

    /// Revokes the credential at the service. The local copy is removed whatever this returns.
    fn revoke(&self, credential: &StoredCredential) -> Result<(), ProviderError>;
}

/// The `state` and PKCE (RFC 7636, S256) values of one browser authorization request.
pub struct BrowserAuthorization {
    state: String,
    verifier: Secret,
}

impl fmt::Debug for BrowserAuthorization {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("BrowserAuthorization([redacted])")
    }
}

impl BrowserAuthorization {
    pub fn new() -> Result<Self, AuthorizationError> {
        Ok(Self {
            state: random_token()?,
            verifier: Secret::from(random_token()?),
        })
    }

    pub fn state(&self) -> &str {
        &self.state
    }

    /// Sent to the token endpoint with the authorization code.
    pub fn code_verifier(&self) -> &Secret {
        &self.verifier
    }

    pub fn code_challenge(&self) -> String {
        URL_SAFE_NO_PAD.encode(Sha256::digest(self.verifier.expose()))
    }

    /// Adds `state`, `code_challenge` and `code_challenge_method` to an authorization URL. The
    /// provider adds its own client ID, scope, response type and redirect URI.
    pub fn apply(&self, authorization_url: &mut Url) {
        authorization_url
            .query_pairs_mut()
            .append_pair("state", &self.state)
            .append_pair("code_challenge", &self.code_challenge())
            .append_pair("code_challenge_method", "S256");
    }

    /// Checks a redirect's query parameters and returns the authorization code.
    ///
    /// A missing, repeated or different `state` is [`AuthorizationError::StateMismatch`]: the
    /// response belongs to another request and must not end this one.
    pub fn validate<'a>(
        &self,
        query: impl IntoIterator<Item = (Cow<'a, str>, Cow<'a, str>)>,
    ) -> Result<Secret, AuthorizationError> {
        let mut states = 0;
        let mut state_matches = false;
        let mut code = None;
        let mut error = false;
        for (key, value) in query {
            match key.as_ref() {
                "state" => {
                    states += 1;
                    state_matches = constant_time_eq(value.as_bytes(), self.state.as_bytes());
                }
                "code" => code = Some(Secret::new(value.as_bytes())),
                "error" => error = true,
                _ => {}
            }
        }
        if states != 1 || !state_matches {
            return Err(AuthorizationError::StateMismatch);
        }
        if error {
            return Err(AuthorizationError::Denied);
        }
        code.filter(|code| !code.is_empty())
            .ok_or(AuthorizationError::MissingCode)
    }
}

/// 256 random bits, base64url without padding (43 characters).
fn random_token() -> Result<String, AuthorizationError> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|_| AuthorizationError::Random)?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}
