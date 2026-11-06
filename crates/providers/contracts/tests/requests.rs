//! Superseded, cancelled and signed-out requests drop their results.

use std::sync::Arc;

use parabolica_domain::provider::UnavailableReason;
use parabolica_provider_contracts::account::{Account, Connection};
use parabolica_provider_contracts::credentials::{
    CredentialStore, MemoryCredentialStore, StoredCredential,
};
use parabolica_provider_contracts::error::ProviderError;
use parabolica_provider_contracts::secret::Secret;
use parabolica_provider_contracts::service::{AccountIdentity, ServiceId};

const SERVICE: ServiceId = ServiceId::YOUTUBE_MUSIC;

fn credential() -> StoredCredential {
    StoredCredential {
        account: AccountIdentity {
            id: "account-1".to_owned(),
            display_name: "Test account".to_owned(),
        },
        secret: Secret::new("disposable-secret"),
    }
}

fn connected() -> (Account, Arc<MemoryCredentialStore>) {
    let store = Arc::new(MemoryCredentialStore::new());
    store.save(SERVICE, &credential()).unwrap();
    let account = Account::new(SERVICE, store.clone(), None);
    account.restore();
    (account, store)
}

fn connected_account() -> Account {
    connected().0
}

#[test]
fn a_newer_request_cancels_and_supersedes_the_older_one() {
    let account = connected_account();
    let searches = account.requests();

    let first = searches.begin();
    let second = searches.begin();
    assert!(first.is_cancelled());
    assert!(first.check().unwrap_err().is_cancelled());
    assert!(!second.is_cancelled());

    assert_eq!(searches.accept(first.tag("old results")), None);
    assert_eq!(
        searches.accept(second.tag("new results")),
        Some("new results")
    );
}

#[test]
fn cancelling_drops_the_current_result() {
    let account = connected_account();
    let searches = account.requests();
    let request = searches.begin();
    searches.cancel();
    assert!(request.is_cancelled());
    assert_eq!(searches.accept(request.tag(1)), None);
}

#[test]
fn scopes_are_independent() {
    let account = connected_account();
    let searches = account.requests();
    let library = account.requests();
    let search = searches.begin();
    let page = library.begin();
    searches.begin();
    assert!(search.is_cancelled());
    assert!(!page.is_cancelled());
    assert_eq!(library.accept(page.tag("page")), Some("page"));
}

#[test]
fn signing_out_cancels_in_flight_requests_and_drops_their_results() {
    let account = connected_account();
    let searches = account.requests();
    let request = searches.begin();

    account.sign_out();
    assert!(request.is_cancelled());
    assert_eq!(
        searches.accept(request.tag("fetched with the old sign-in")),
        None
    );

    // A request made after sign-out belongs to the new session.
    let after = searches.begin();
    assert!(!after.is_cancelled());
    assert_eq!(
        searches.accept(after.tag("public results")),
        Some("public results")
    );
}

#[test]
fn an_expiry_reported_by_an_old_session_does_not_expire_the_new_one() {
    let (account, store) = connected();
    let searches = account.requests();
    let old = searches.begin();
    let expired = ProviderError::unavailable(SERVICE, UnavailableReason::SessionExpired);

    // Sign out, then connect again; a late failure from the first session arrives.
    account.sign_out();
    store.save(SERVICE, &credential()).unwrap();
    account.restore();
    assert_eq!(account.report(&old, &expired), None);
    assert!(matches!(
        account.status().state.connection,
        Connection::Connected(_)
    ));

    let current = searches.begin();
    let snapshot = account.report(&current, &expired).unwrap();
    assert!(matches!(snapshot.state.connection, Connection::Expired(_)));
}

#[test]
fn only_session_ending_errors_expire_the_account() {
    let account = connected_account();
    let request = account.requests().begin();
    let offline = ProviderError::unavailable(SERVICE, UnavailableReason::Offline);
    assert_eq!(account.report(&request, &offline), None);
    assert!(matches!(
        account.status().state.connection,
        Connection::Connected(_)
    ));
}

#[test]
fn a_request_from_before_a_restore_cannot_expire_the_restored_session() {
    let store = Arc::new(MemoryCredentialStore::new());
    let account = Account::new(SERVICE, store.clone(), None);
    let signed_out_request = account.requests().begin();
    store.save(SERVICE, &credential()).unwrap();
    account.restore();

    let rejected = ProviderError::unavailable(SERVICE, UnavailableReason::SignedOut);
    assert_eq!(account.report(&signed_out_request, &rejected), None);
    assert!(matches!(
        account.status().state.connection,
        Connection::Connected(_)
    ));
}

#[test]
fn another_services_request_cannot_expire_this_account() {
    let (account, _) = connected();
    let other = Account::new(
        ServiceId::DISCOGS,
        Arc::new(MemoryCredentialStore::new()),
        None,
    );
    let request = other.requests().begin();
    let expired = ProviderError::unavailable(SERVICE, UnavailableReason::SessionExpired);
    assert_eq!(account.report(&request, &expired), None);
}
