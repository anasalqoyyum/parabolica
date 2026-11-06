//! Account lifecycle: restore, sign-in, reconnect, expiry and sign-out against a fake store and
//! flow. Secrets are disposable test strings.

use std::sync::{Arc, Mutex};

use parabolica_domain::provider::UnavailableReason;
use parabolica_provider_contracts::account::{Account, AccountEvent, AccountProblem, Connection};
use parabolica_provider_contracts::auth::{
    AuthAttempt, AuthFlow, AuthorizationError, Browser, SignedIn,
};
use parabolica_provider_contracts::credentials::{
    CredentialError, CredentialStore, MemoryCredentialStore, StoredCredential,
};
use parabolica_provider_contracts::error::{ProviderError, ProviderErrorKind};
use parabolica_provider_contracts::secret::Secret;
use parabolica_provider_contracts::service::{AccountIdentity, ServiceId};
use url::Url;

const SERVICE: ServiceId = ServiceId::YOUTUBE_MUSIC;

fn identity(id: &str) -> AccountIdentity {
    AccountIdentity {
        id: id.to_owned(),
        display_name: format!("Account {id}"),
    }
}

fn signed_in(id: &str, secret: &str) -> SignedIn {
    SignedIn {
        account: identity(id),
        secret: Secret::new(secret),
    }
}

/// Returns queued sign-in results in order and records revocations.
#[derive(Debug, Default)]
struct ScriptedFlow {
    results: Mutex<Vec<Result<SignedIn, ProviderError>>>,
    revoke_failure: Mutex<Option<ProviderError>>,
    revoked: Mutex<Vec<String>>,
}

impl ScriptedFlow {
    fn then(&self, result: Result<SignedIn, ProviderError>) {
        self.results.lock().unwrap().insert(0, result);
    }
}

impl AuthFlow for ScriptedFlow {
    fn sign_in(&self, _: &AuthAttempt, _: &dyn Browser) -> Result<SignedIn, ProviderError> {
        self.results
            .lock()
            .unwrap()
            .pop()
            .expect("no scripted result")
    }

    fn revoke(&self, credential: &StoredCredential) -> Result<(), ProviderError> {
        self.revoked
            .lock()
            .unwrap()
            .push(credential.account.id.clone());
        match self.revoke_failure.lock().unwrap().clone() {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }
}

#[derive(Debug)]
struct NoBrowser;

impl Browser for NoBrowser {
    fn open(&self, _: &Url) -> Result<(), AuthorizationError> {
        Ok(())
    }
}

struct Fixture {
    store: Arc<MemoryCredentialStore>,
    flow: Arc<ScriptedFlow>,
    account: Account,
}

fn fixture() -> Fixture {
    let store = Arc::new(MemoryCredentialStore::new());
    let flow = Arc::new(ScriptedFlow::default());
    let account = Account::new(SERVICE, store.clone(), Some(flow.clone()));
    Fixture {
        store,
        flow,
        account,
    }
}

fn saved_secret(store: &MemoryCredentialStore) -> Option<Vec<u8>> {
    store
        .load(SERVICE)
        .unwrap()
        .map(|credential| credential.secret.expose().to_vec())
}

#[test]
fn a_saved_sign_in_is_restored_without_signing_in_again() {
    let Fixture { store, account, .. } = fixture();
    store
        .save(
            SERVICE,
            &StoredCredential {
                account: identity("a"),
                secret: Secret::new("saved-secret"),
            },
        )
        .unwrap();
    let events = account.subscribe();

    let status = account.restore().state;
    assert_eq!(status.connection, Connection::Connected(identity("a")));
    assert_eq!(
        events.try_recv(),
        Ok(AccountEvent::Connected(identity("a")))
    );
}

#[test]
fn an_unreadable_store_is_reported_and_leaves_the_account_signed_out() {
    let Fixture { store, account, .. } = fixture();
    store.fail_with(Some(CredentialError::Unavailable));
    let status = account.restore().state;
    assert_eq!(status.connection, Connection::SignedOut);
    assert_eq!(
        status.problem,
        Some(AccountProblem::Restore(ProviderError::new(
            SERVICE,
            CredentialError::Unavailable
        )))
    );
}

#[test]
fn signing_in_saves_the_credential_and_announces_the_account() {
    let Fixture {
        store,
        flow,
        account,
    } = fixture();
    let events = account.subscribe();
    flow.then(Ok(signed_in("a", "secret-a")));

    let (attempt, started) = account.begin_sign_in().unwrap();
    assert!(started.state.signing_in);
    assert_eq!(attempt.reconnecting(), None);

    let status = account.sign_in(&attempt, &NoBrowser).unwrap().state;
    assert_eq!(status.connection, Connection::Connected(identity("a")));
    assert!(!status.signing_in);
    assert_eq!(saved_secret(&store), Some(b"secret-a".to_vec()));
    assert_eq!(
        events.try_recv(),
        Ok(AccountEvent::Connected(identity("a")))
    );
}

#[test]
fn a_superseded_attempt_cannot_connect_or_save() {
    let Fixture { store, account, .. } = fixture();
    let (first, _) = account.begin_sign_in().unwrap();
    let (second, _) = account.begin_sign_in().unwrap();
    assert!(first.is_cancelled());

    assert_eq!(
        account.finish_sign_in(&first, Ok(signed_in("stale", "stale-secret"))),
        None
    );
    assert_eq!(saved_secret(&store), None);
    assert!(account.status().state.signing_in);

    let status = account
        .finish_sign_in(&second, Ok(signed_in("b", "secret-b")))
        .unwrap()
        .state;
    assert_eq!(status.connection, Connection::Connected(identity("b")));
    // The same result cannot be applied twice.
    assert_eq!(
        account.finish_sign_in(&second, Ok(signed_in("c", "secret-c"))),
        None
    );
    assert_eq!(saved_secret(&store), Some(b"secret-b".to_vec()));
}

#[test]
fn a_cancelled_attempt_cannot_connect_later() {
    let Fixture { store, account, .. } = fixture();
    let (attempt, _) = account.begin_sign_in().unwrap();
    let status = account.cancel_sign_in().state;
    assert!(!status.signing_in);
    assert!(attempt.is_cancelled());
    assert_eq!(
        account.finish_sign_in(&attempt, Ok(signed_in("a", "late-secret"))),
        None
    );
    assert_eq!(saved_secret(&store), None);
    assert_eq!(account.status().state.connection, Connection::SignedOut);
}

#[test]
fn signing_out_during_a_sign_in_discards_its_result() {
    let Fixture { store, account, .. } = fixture();
    let (attempt, _) = account.begin_sign_in().unwrap();
    account.sign_out();
    assert_eq!(
        account.finish_sign_in(&attempt, Ok(signed_in("a", "late-secret"))),
        None
    );
    assert_eq!(saved_secret(&store), None);
}

#[test]
fn reconnecting_an_expired_account_requires_the_same_account() {
    let Fixture {
        store,
        flow,
        account,
    } = fixture();
    flow.then(Ok(signed_in("a", "secret-a")));
    let (attempt, _) = account.begin_sign_in().unwrap();
    account.sign_in(&attempt, &NoBrowser);
    let request = account.requests().begin();
    account.report(
        &request,
        &ProviderError::unavailable(SERVICE, UnavailableReason::SessionExpired),
    );
    assert_eq!(
        account.status().state.connection,
        Connection::Expired(identity("a"))
    );

    flow.then(Ok(signed_in("intruder", "secret-intruder")));
    let (attempt, _) = account.begin_sign_in().unwrap();
    assert_eq!(attempt.reconnecting(), Some(&identity("a")));
    let status = account.sign_in(&attempt, &NoBrowser).unwrap().state;
    assert_eq!(status.connection, Connection::Expired(identity("a")));
    assert_eq!(
        status.problem,
        Some(AccountProblem::SignIn(ProviderError::new(
            SERVICE,
            AuthorizationError::WrongAccount
        )))
    );
    assert_eq!(saved_secret(&store), Some(b"secret-a".to_vec()));

    flow.then(Ok(signed_in("a", "secret-a-renewed")));
    let (attempt, _) = account.begin_sign_in().unwrap();
    let status = account.sign_in(&attempt, &NoBrowser).unwrap().state;
    assert_eq!(status.connection, Connection::Connected(identity("a")));
    assert_eq!(status.problem, None);
    assert_eq!(saved_secret(&store), Some(b"secret-a-renewed".to_vec()));
}

#[test]
fn reconnecting_ends_the_old_session() {
    let Fixture { flow, account, .. } = fixture();
    flow.then(Ok(signed_in("a", "secret-a")));
    let (attempt, _) = account.begin_sign_in().unwrap();
    account.sign_in(&attempt, &NoBrowser);
    let searches = account.requests();
    let before = searches.begin();

    flow.then(Ok(signed_in("a", "secret-a-renewed")));
    let (attempt, _) = account.begin_sign_in().unwrap();
    account.sign_in(&attempt, &NoBrowser);
    assert!(before.is_cancelled());
    assert_eq!(searches.accept(before.tag("old")), None);
}

#[test]
fn failed_and_cancelled_sign_ins_change_nothing_else() {
    let Fixture {
        store,
        flow,
        account,
    } = fixture();
    flow.then(Err(ProviderError::new(SERVICE, AuthorizationError::Denied)));
    let (attempt, _) = account.begin_sign_in().unwrap();
    let status = account.sign_in(&attempt, &NoBrowser).unwrap().state;
    assert_eq!(status.connection, Connection::SignedOut);
    assert_eq!(
        status.problem,
        Some(AccountProblem::SignIn(ProviderError::new(
            SERVICE,
            AuthorizationError::Denied
        )))
    );

    flow.then(Err(ProviderError::new(
        SERVICE,
        ProviderErrorKind::Cancelled,
    )));
    let (attempt, _) = account.begin_sign_in().unwrap();
    let status = account.sign_in(&attempt, &NoBrowser).unwrap().state;
    assert_eq!(status.problem, None, "a cancelled sign-in is not a failure");
    assert_eq!(saved_secret(&store), None);
}

#[test]
fn a_credential_that_cannot_be_saved_does_not_connect() {
    let Fixture {
        store,
        flow,
        account,
    } = fixture();
    flow.then(Ok(signed_in("a", "secret-a")));
    let (attempt, _) = account.begin_sign_in().unwrap();
    store.fail_with(Some(CredentialError::Os {
        operation: "save",
        code: 5,
    }));
    let status = account.sign_in(&attempt, &NoBrowser).unwrap().state;
    assert_eq!(status.connection, Connection::SignedOut);
    assert!(matches!(status.problem, Some(AccountProblem::SignIn(_))));
}

#[test]
fn signing_out_removes_the_secret_revokes_it_and_stops_the_session() {
    let Fixture {
        store,
        flow,
        account,
    } = fixture();
    flow.then(Ok(signed_in("a", "secret-a")));
    let (attempt, _) = account.begin_sign_in().unwrap();
    account.sign_in(&attempt, &NoBrowser);
    let events = account.subscribe();
    let request = account.requests().begin();

    let status = account.sign_out().state;
    assert_eq!(status.connection, Connection::SignedOut);
    assert_eq!(status.problem, None);
    assert!(!store.contains(SERVICE));
    assert!(request.is_cancelled());
    assert_eq!(*flow.revoked.lock().unwrap(), ["a"]);
    assert_eq!(events.try_recv(), Ok(AccountEvent::SignedOut));
}

#[test]
fn a_failed_revocation_is_reported_but_the_local_secret_stays_removed() {
    let Fixture {
        store,
        flow,
        account,
    } = fixture();
    flow.then(Ok(signed_in("a", "secret-a")));
    let (attempt, _) = account.begin_sign_in().unwrap();
    account.sign_in(&attempt, &NoBrowser);
    let offline = ProviderError::unavailable(SERVICE, UnavailableReason::Offline);
    *flow.revoke_failure.lock().unwrap() = Some(offline.clone());

    let status = account.sign_out().state;
    assert_eq!(status.connection, Connection::SignedOut);
    assert_eq!(status.problem, Some(AccountProblem::Revoke(offline)));
    assert!(!store.contains(SERVICE));
}

#[test]
fn a_secret_that_cannot_be_removed_keeps_the_account_connected() {
    let Fixture {
        store,
        flow,
        account,
    } = fixture();
    flow.then(Ok(signed_in("a", "secret-a")));
    let (attempt, _) = account.begin_sign_in().unwrap();
    account.sign_in(&attempt, &NoBrowser);
    let events = account.subscribe();
    let failure = CredentialError::Os {
        operation: "remove",
        code: 5,
    };
    store.fail_with(Some(failure.clone()));

    let status = account.sign_out().state;
    assert_eq!(status.connection, Connection::Connected(identity("a")));
    assert_eq!(
        status.problem,
        Some(AccountProblem::Remove(ProviderError::new(SERVICE, failure)))
    );
    assert!(events.try_recv().is_err());
    assert!(flow.revoked.lock().unwrap().is_empty());

    store.fail_with(None);
    assert_eq!(account.sign_out().state.connection, Connection::SignedOut);
    assert!(!store.contains(SERVICE));
}

#[test]
fn an_account_without_a_flow_restores_and_signs_out_but_cannot_sign_in() {
    let store = Arc::new(MemoryCredentialStore::new());
    store
        .save(
            SERVICE,
            &StoredCredential {
                account: identity("a"),
                secret: Secret::new("saved-secret"),
            },
        )
        .unwrap();
    let account = Account::new(SERVICE, store.clone(), None);
    assert!(!account.can_sign_in());
    assert!(account.begin_sign_in().is_none());
    account.restore();
    assert_eq!(account.sign_out().state.connection, Connection::SignedOut);
    assert!(!store.contains(SERVICE));
}

#[test]
fn snapshots_are_versioned_in_order() {
    let Fixture { account, .. } = fixture();
    let first = account.status();
    let (_, second) = account.begin_sign_in().unwrap();
    let third = account.cancel_sign_in();
    assert!(first.version < second.version && second.version < third.version);
}

#[test]
fn changed_wakes_watchers_when_a_controller_reports_expiry() {
    use std::future::Future as _;
    use std::pin::pin;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::task::{Context, Poll, Wake, Waker};

    struct Flag(AtomicBool);
    impl Wake for Flag {
        fn wake(self: Arc<Self>) {
            self.0.store(true, Ordering::SeqCst);
        }
    }

    let Fixture { store, account, .. } = fixture();
    store
        .save(
            SERVICE,
            &StoredCredential {
                account: identity("a"),
                secret: Secret::new("saved-secret"),
            },
        )
        .unwrap();
    account.restore();
    let seen = account.status().version;
    let flag = Arc::new(Flag(AtomicBool::new(false)));
    let waker = Waker::from(flag.clone());
    let mut context = Context::from_waker(&waker);
    let mut changed = pin!(account.changed(seen));
    assert_eq!(changed.as_mut().poll(&mut context), Poll::Pending);
    assert_eq!(
        account.status().version,
        seen,
        "reading status publishes nothing"
    );

    let request = account.requests().begin();
    account.report(
        &request,
        &ProviderError::unavailable(SERVICE, UnavailableReason::SessionExpired),
    );
    assert!(flag.0.load(Ordering::SeqCst));
    assert_eq!(changed.as_mut().poll(&mut context), Poll::Ready(()));
    assert_eq!(
        account.status().state.connection,
        Connection::Expired(identity("a"))
    );
}

/// Signs in as "a"; `revoke` waits until the test releases it.
#[derive(Debug)]
struct SlowRevoke {
    started: Mutex<std::sync::mpsc::Sender<()>>,
    release: Mutex<std::sync::mpsc::Receiver<()>>,
}

impl AuthFlow for SlowRevoke {
    fn sign_in(&self, _: &AuthAttempt, _: &dyn Browser) -> Result<SignedIn, ProviderError> {
        Ok(signed_in("a", "secret-a"))
    }

    fn revoke(&self, _: &StoredCredential) -> Result<(), ProviderError> {
        self.started.lock().unwrap().send(()).unwrap();
        self.release.lock().unwrap().recv().unwrap();
        Err(ProviderError::unavailable(
            SERVICE,
            UnavailableReason::Offline,
        ))
    }
}

#[test]
fn sign_out_is_visible_before_a_slow_revocation_finishes() {
    let (started_tx, started) = std::sync::mpsc::channel();
    let (release, release_rx) = std::sync::mpsc::channel();
    let store = Arc::new(MemoryCredentialStore::new());
    let account = Arc::new(Account::new(
        SERVICE,
        store.clone(),
        Some(Arc::new(SlowRevoke {
            started: Mutex::new(started_tx),
            release: Mutex::new(release_rx),
        })),
    ));
    let (attempt, _) = account.begin_sign_in().unwrap();
    account.sign_in(&attempt, &NoBrowser);
    assert!(matches!(
        account.status().state.connection,
        Connection::Connected(_)
    ));
    let events = account.subscribe();

    let signing_out = {
        let account = account.clone();
        std::thread::spawn(move || account.sign_out())
    };
    started.recv().unwrap();
    assert_eq!(account.status().state.connection, Connection::SignedOut);
    assert_eq!(account.status().state.problem, None);
    assert_eq!(events.try_recv(), Ok(AccountEvent::SignedOut));
    assert!(!store.contains(SERVICE));

    release.send(()).unwrap();
    let status = signing_out.join().unwrap().state;
    assert!(matches!(status.problem, Some(AccountProblem::Revoke(_))));
}
