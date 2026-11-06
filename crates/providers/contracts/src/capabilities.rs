//! What a provider can do under the current account and network state.

use std::collections::BTreeMap;
use std::time::Duration;

use parabolica_domain::provider::{
    Availability, Capability, ProviderCapabilities, ProviderId, UnavailableReason,
};

use crate::account::Connection;

/// Whether a capability needs a signed-in account.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Access {
    /// Works signed out, such as public catalog search.
    Public,
    Account,
}

/// Whether the service can be reached at all.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Reachability {
    #[default]
    Online,
    Offline,
    Restricted(Restriction),
}

/// The service refuses this client or region, independent of the account.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Restriction {
    Region,
    RateLimited { retry_after: Option<Duration> },
    Blocked(String),
}

impl Restriction {
    fn reason(&self) -> UnavailableReason {
        match self {
            Self::Region => UnavailableReason::RegionRestricted,
            Self::RateLimited { retry_after } => UnavailableReason::RateLimited {
                retry_after: *retry_after,
            },
            Self::Blocked(reason) => UnavailableReason::Blocked(reason.clone()),
        }
    }
}

/// The capabilities a provider implements and whether each needs an account. A capability the
/// provider does not declare is reported as `NotSupported` in every state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderSupport {
    provider: ProviderId,
    declared: BTreeMap<Capability, Access>,
}

impl ProviderSupport {
    pub fn new(provider: ProviderId) -> Self {
        Self {
            provider,
            declared: BTreeMap::new(),
        }
    }

    pub fn declare(mut self, capability: Capability, access: Access) -> Self {
        self.declared.insert(capability, access);
        self
    }

    pub fn provider(&self) -> ProviderId {
        self.provider
    }

    /// Availability of every declared capability. An unreachable service makes all of them
    /// unavailable for that reason; otherwise account capabilities follow the connection and
    /// public ones stay available.
    pub fn capabilities(
        &self,
        connection: &Connection,
        reachability: &Reachability,
    ) -> ProviderCapabilities {
        let mut capabilities = ProviderCapabilities::new(self.provider);
        for (&capability, &access) in &self.declared {
            let availability = match (reachability, access, connection) {
                (Reachability::Offline, _, _) => {
                    Availability::Unavailable(UnavailableReason::Offline)
                }
                (Reachability::Restricted(restriction), _, _) => {
                    Availability::Unavailable(restriction.reason())
                }
                (Reachability::Online, Access::Public, _)
                | (Reachability::Online, Access::Account, Connection::Connected(_)) => {
                    Availability::Available
                }
                (Reachability::Online, Access::Account, Connection::SignedOut) => {
                    Availability::Unavailable(UnavailableReason::SignedOut)
                }
                (Reachability::Online, Access::Account, Connection::Expired(_)) => {
                    Availability::Unavailable(UnavailableReason::SessionExpired)
                }
            };
            capabilities.set(capability, availability);
        }
        capabilities
    }
}
