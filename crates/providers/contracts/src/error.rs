//! Errors scoped to the service that produced them.

use std::fmt;

use parabolica_domain::provider::UnavailableReason;

use crate::auth::AuthorizationError;
use crate::credentials::CredentialError;
use crate::service::ServiceId;

/// A failure from one service. The UI shows it beside that service's account or items only.
///
/// Messages never contain secrets, cookies or signed URLs. Adapters pass URLs through
/// [`crate::secret::redact_url`] before putting them in [`ProviderErrorKind::Protocol`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{service}: {kind}")]
pub struct ProviderError {
    pub service: ServiceId,
    pub kind: ProviderErrorKind,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ProviderErrorKind {
    #[error("{}", Reason(.0))]
    Unavailable(UnavailableReason),
    #[error("cancelled")]
    Cancelled,
    #[error(transparent)]
    Authorization(#[from] AuthorizationError),
    #[error(transparent)]
    Credential(#[from] CredentialError),
    #[error("unexpected response: {detail}")]
    Protocol { detail: String },
}

impl ProviderError {
    pub fn new(service: ServiceId, kind: impl Into<ProviderErrorKind>) -> Self {
        Self {
            service,
            kind: kind.into(),
        }
    }

    pub fn unavailable(service: ServiceId, reason: UnavailableReason) -> Self {
        Self::new(service, ProviderErrorKind::Unavailable(reason))
    }

    pub fn is_cancelled(&self) -> bool {
        self.kind == ProviderErrorKind::Cancelled
    }

    /// Whether the service rejected the saved sign-in, so the account needs a reconnect.
    pub fn ends_session(&self) -> bool {
        matches!(
            self.kind,
            ProviderErrorKind::Unavailable(
                UnavailableReason::SessionExpired | UnavailableReason::SignedOut
            )
        )
    }
}

struct Reason<'a>(&'a UnavailableReason);

impl fmt::Display for Reason<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            UnavailableReason::NotSupported => f.write_str("not supported"),
            UnavailableReason::SignedOut => f.write_str("signed out"),
            UnavailableReason::SessionExpired => f.write_str("session expired"),
            UnavailableReason::Offline => f.write_str("offline"),
            UnavailableReason::RegionRestricted => f.write_str("not available in this region"),
            UnavailableReason::RateLimited {
                retry_after: Some(after),
            } => write!(f, "rate limited, retry in {} s", after.as_secs().max(1)),
            UnavailableReason::RateLimited { retry_after: None } => f.write_str("rate limited"),
            UnavailableReason::Blocked(reason) => write!(f, "blocked: {reason}"),
        }
    }
}
