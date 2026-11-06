//! Capabilities reported for ready, signed-out, expired, offline and restricted sessions.

use std::time::Duration;

use parabolica_domain::provider::{
    Availability, Capability, PlaybackOwner, ProviderId, UnavailableReason,
};
use parabolica_provider_contracts::account::Connection;
use parabolica_provider_contracts::capabilities::{
    Access, ProviderSupport, Reachability, Restriction,
};
use parabolica_provider_contracts::service::AccountIdentity;

fn account() -> AccountIdentity {
    AccountIdentity {
        id: "account-1".to_owned(),
        display_name: "Test account".to_owned(),
    }
}

/// Public search, account-only library and playback; no seek, no app-owned PCM.
fn support() -> ProviderSupport {
    ProviderSupport::new(ProviderId::YouTubeMusic)
        .declare(Capability::Search, Access::Public)
        .declare(Capability::LibraryRead, Access::Account)
        .declare(Capability::Playback, Access::Account)
}

fn unavailable(reason: UnavailableReason) -> Availability {
    Availability::Unavailable(reason)
}

const UNDECLARED: [Capability; 3] = [
    Capability::Seek,
    Capability::AppOwnedPcm,
    Capability::Gapless,
];

#[test]
fn ready_session_reports_declared_capabilities_only() {
    let capabilities =
        support().capabilities(&Connection::Connected(account()), &Reachability::Online);
    assert!(capabilities.is_available(Capability::Search));
    assert!(capabilities.is_available(Capability::LibraryRead));
    assert!(capabilities.is_available(Capability::Playback));
    for capability in UNDECLARED {
        assert_eq!(
            capabilities.availability(capability),
            unavailable(UnavailableReason::NotSupported),
            "{capability:?}"
        );
    }
    assert_eq!(
        capabilities.playback_owner(),
        PlaybackOwner::Provider(ProviderId::YouTubeMusic)
    );
}

#[test]
fn signed_out_keeps_public_capabilities_and_names_the_reason_for_account_ones() {
    let capabilities = support().capabilities(&Connection::SignedOut, &Reachability::Online);
    assert!(capabilities.is_available(Capability::Search));
    assert_eq!(
        capabilities.availability(Capability::LibraryRead),
        unavailable(UnavailableReason::SignedOut)
    );
    assert_eq!(
        capabilities.availability(Capability::Playback),
        unavailable(UnavailableReason::SignedOut)
    );
    for capability in UNDECLARED {
        assert_eq!(
            capabilities.availability(capability),
            unavailable(UnavailableReason::NotSupported),
            "an unsupported capability is not a sign-in problem"
        );
    }
}

#[test]
fn expired_session_reports_session_expired_not_signed_out() {
    let capabilities =
        support().capabilities(&Connection::Expired(account()), &Reachability::Online);
    assert!(capabilities.is_available(Capability::Search));
    assert_eq!(
        capabilities.availability(Capability::Playback),
        unavailable(UnavailableReason::SessionExpired)
    );
    assert_eq!(
        capabilities.availability(Capability::Seek),
        unavailable(UnavailableReason::NotSupported)
    );
}

#[test]
fn offline_makes_every_declared_capability_unavailable_in_every_account_state() {
    for connection in [
        Connection::Connected(account()),
        Connection::SignedOut,
        Connection::Expired(account()),
    ] {
        let capabilities = support().capabilities(&connection, &Reachability::Offline);
        for capability in [
            Capability::Search,
            Capability::LibraryRead,
            Capability::Playback,
        ] {
            assert_eq!(
                capabilities.availability(capability),
                unavailable(UnavailableReason::Offline),
                "{connection:?} {capability:?}"
            );
        }
        for capability in UNDECLARED {
            assert_eq!(
                capabilities.availability(capability),
                unavailable(UnavailableReason::NotSupported)
            );
        }
    }
}

#[test]
fn restrictions_carry_their_reason() {
    let cases = [
        (Restriction::Region, UnavailableReason::RegionRestricted),
        (
            Restriction::RateLimited {
                retry_after: Some(Duration::from_secs(30)),
            },
            UnavailableReason::RateLimited {
                retry_after: Some(Duration::from_secs(30)),
            },
        ),
        (
            Restriction::Blocked("client blocked".to_owned()),
            UnavailableReason::Blocked("client blocked".to_owned()),
        ),
    ];
    for (restriction, reason) in cases {
        let capabilities = support().capabilities(
            &Connection::Connected(account()),
            &Reachability::Restricted(restriction),
        );
        assert_eq!(
            capabilities.availability(Capability::Search),
            unavailable(reason.clone())
        );
        assert_eq!(
            capabilities.availability(Capability::Playback),
            unavailable(reason)
        );
        assert_eq!(
            capabilities.availability(Capability::Seek),
            unavailable(UnavailableReason::NotSupported)
        );
    }
}

#[test]
fn app_owned_pcm_moves_playback_to_the_native_engine_only_while_available() {
    let support = ProviderSupport::new(ProviderId::YouTubeMusic)
        .declare(Capability::Playback, Access::Account)
        .declare(Capability::AppOwnedPcm, Access::Account);
    let ready = support.capabilities(&Connection::Connected(account()), &Reachability::Online);
    assert_eq!(ready.playback_owner(), PlaybackOwner::NativeEngine);
    let expired = support.capabilities(&Connection::Expired(account()), &Reachability::Online);
    assert_eq!(
        expired.playback_owner(),
        PlaybackOwner::Provider(ProviderId::YouTubeMusic)
    );
}
