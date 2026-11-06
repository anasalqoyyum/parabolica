//! The account lifecycle of one service.
//!
//! [`Account`] owns the connection state, runs the provider's [`AuthFlow`], saves and removes
//! the credential in the [`CredentialStore`] and ends the session's requests on sign-out or
//! reconnect. Every change publishes a new [`Versioned`] [`AccountStatus`]; the UI awaits
//! [`Account::changed`] and applies [`Account::status`] through a `SnapshotGate`. Lifecycle
//! changes also go to [`AccountEvent`] subscribers such as playback and scrobbling controllers.
//!
//! Methods that touch the store or the network block; call them off the UI thread.

use std::future::Future;
use std::pin::Pin;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};

use parabolica_domain::control::{
    CancelToken, GenerationCounter, SnapshotSequencer, SnapshotVersion, Versioned,
};

use crate::auth::{AuthAttempt, AuthFlow, AuthorizationError, Browser, SignedIn};
use crate::credentials::{CredentialStore, StoredCredential};
use crate::error::ProviderError;
use crate::requests::{Request, RequestScope, SessionGate};
use crate::service::{AccountIdentity, ServiceId};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Connection {
    SignedOut,
    Connected(AccountIdentity),
    /// The service rejected the saved sign-in. The credential stays saved until the user
    /// reconnects the same account or disconnects.
    Expired(AccountIdentity),
}

impl Connection {
    pub fn account(&self) -> Option<&AccountIdentity> {
        match self {
            Self::SignedOut => None,
            Self::Connected(account) | Self::Expired(account) => Some(account),
        }
    }
}

/// The last failure, kept until the next account action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AccountProblem {
    /// The saved sign-in could not be read at startup.
    Restore(ProviderError),
    SignIn(ProviderError),
    /// The saved sign-in could not be removed; the account is still connected.
    Remove(ProviderError),
    /// The local sign-in was removed, but the service did not confirm revoking it.
    Revoke(ProviderError),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountStatus {
    pub service: ServiceId,
    pub connection: Connection,
    pub signing_in: bool,
    pub problem: Option<AccountProblem>,
}

/// Lifecycle changes for controllers that hold provider sessions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AccountEvent {
    /// A saved sign-in was restored or a new one accepted.
    Connected(AccountIdentity),
    Expired(AccountIdentity),
    /// Stop provider playback and pending work for this service.
    SignedOut,
}

#[derive(Debug)]
pub struct Account {
    service: ServiceId,
    store: Arc<dyn CredentialStore>,
    flow: Option<Arc<dyn AuthFlow>>,
    session: Arc<SessionGate>,
    attempts: GenerationCounter,
    /// Held across credential store calls, so sign-in, sign-out and expiry apply atomically.
    state: Mutex<State>,
    /// Held only briefly, so the UI can read the status while the store is busy.
    published: Mutex<Published>,
}

#[derive(Debug)]
struct State {
    status: AccountStatus,
    snapshots: SnapshotSequencer,
    attempt: Option<CancelToken>,
    subscribers: Vec<Sender<AccountEvent>>,
}

#[derive(Debug)]
struct Published {
    snapshot: Versioned<AccountStatus>,
    watchers: Vec<Waker>,
}

impl State {
    /// Publishes the current status as a new snapshot and wakes [`Changed`] futures.
    fn publish(&mut self, published: &Mutex<Published>) -> Versioned<AccountStatus> {
        let snapshot = self.snapshots.wrap(self.status.clone());
        let mut published = published.lock().unwrap();
        published.snapshot = snapshot.clone();
        for watcher in published.watchers.drain(..) {
            watcher.wake();
        }
        snapshot
    }

    fn emit(&mut self, event: AccountEvent) {
        self.subscribers
            .retain(|subscriber| subscriber.send(event.clone()).is_ok());
    }

    fn cancel_attempt(&mut self, attempts: &GenerationCounter) {
        if let Some(attempt) = self.attempt.take() {
            attempt.cancel();
        }
        attempts.advance();
        self.status.signing_in = false;
    }
}

impl Account {
    /// `flow` is `None` until the provider implements sign-in; the account can still restore and
    /// remove a saved credential.
    pub fn new(
        service: ServiceId,
        store: Arc<dyn CredentialStore>,
        flow: Option<Arc<dyn AuthFlow>>,
    ) -> Self {
        let status = AccountStatus {
            service,
            connection: Connection::SignedOut,
            signing_in: false,
            problem: None,
        };
        let mut snapshots = SnapshotSequencer::default();
        Self {
            service,
            store,
            flow,
            session: Arc::default(),
            attempts: GenerationCounter::default(),
            published: Mutex::new(Published {
                snapshot: snapshots.wrap(status.clone()),
                watchers: Vec::new(),
            }),
            state: Mutex::new(State {
                status,
                snapshots,
                attempt: None,
                subscribers: Vec::new(),
            }),
        }
    }

    pub fn service(&self) -> ServiceId {
        self.service
    }

    pub fn can_sign_in(&self) -> bool {
        self.flow.is_some()
    }

    /// The latest published status.
    pub fn status(&self) -> Versioned<AccountStatus> {
        self.published.lock().unwrap().snapshot.clone()
    }

    /// Resolves once a status newer than `seen` has been published.
    pub fn changed(&self, seen: SnapshotVersion) -> Changed<'_> {
        Changed {
            account: self,
            seen,
        }
    }

    pub fn subscribe(&self) -> Receiver<AccountEvent> {
        let (sender, receiver) = mpsc::channel();
        self.state.lock().unwrap().subscribers.push(sender);
        receiver
    }

    /// A request stream bound to this account's session.
    pub fn requests(&self) -> RequestScope {
        RequestScope::new(self.service, self.session.clone())
    }

    /// Loads the saved sign-in. Blocks on the credential store.
    pub fn restore(&self) -> Versioned<AccountStatus> {
        let mut state = self.state.lock().unwrap();
        if state.status.connection == Connection::SignedOut && !state.status.signing_in {
            match self.store.load(self.service) {
                Ok(Some(credential)) => {
                    self.session.end();
                    state.status.connection = Connection::Connected(credential.account.clone());
                    state.emit(AccountEvent::Connected(credential.account));
                }
                Ok(None) => {}
                Err(error) => {
                    state.status.problem = Some(AccountProblem::Restore(ProviderError::new(
                        self.service,
                        error,
                    )));
                }
            }
        }
        state.publish(&self.published)
    }

    /// Starts a sign-in, cancelling any earlier attempt. Returns `None` when the provider has no
    /// sign-in flow. A connected or expired account can only be reconnected as itself.
    pub fn begin_sign_in(&self) -> Option<(AuthAttempt, Versioned<AccountStatus>)> {
        self.flow.as_ref()?;
        let mut state = self.state.lock().unwrap();
        state.cancel_attempt(&self.attempts);
        let cancel = CancelToken::new();
        let attempt = AuthAttempt::new(
            self.service,
            self.attempts.advance(),
            state.status.connection.account().cloned(),
            cancel.clone(),
        );
        state.attempt = Some(cancel);
        state.status.signing_in = true;
        state.status.problem = None;
        Some((attempt, state.publish(&self.published)))
    }

    /// Runs the provider's sign-in for `attempt` and applies the result. Blocks until the
    /// browser flow ends. Returns `None` when a newer attempt or a sign-out superseded it.
    pub fn sign_in(
        &self,
        attempt: &AuthAttempt,
        browser: &dyn Browser,
    ) -> Option<Versioned<AccountStatus>> {
        let result = self.flow.as_ref()?.sign_in(attempt, browser);
        self.finish_sign_in(attempt, result)
    }

    /// Applies a sign-in result. A superseded attempt changes nothing and its secret is dropped.
    /// A reconnect that signed in to a different account is rejected and saves nothing.
    pub fn finish_sign_in(
        &self,
        attempt: &AuthAttempt,
        result: Result<SignedIn, ProviderError>,
    ) -> Option<Versioned<AccountStatus>> {
        let mut state = self.state.lock().unwrap();
        if !self.attempts.is_current(attempt.generation()) {
            return None;
        }
        state.attempt = None;
        state.status.signing_in = false;
        self.attempts.advance();

        match result {
            Err(error) if error.is_cancelled() => {}
            Err(error) => state.status.problem = Some(AccountProblem::SignIn(error)),
            Ok(signed_in) => {
                if attempt
                    .reconnecting()
                    .is_some_and(|expected| expected.id != signed_in.account.id)
                {
                    state.status.problem = Some(AccountProblem::SignIn(
                        attempt.fail(AuthorizationError::WrongAccount),
                    ));
                    return Some(state.publish(&self.published));
                }
                let credential = StoredCredential {
                    account: signed_in.account,
                    secret: signed_in.secret,
                };
                match self.store.save(self.service, &credential) {
                    Ok(()) => {
                        self.session.end();
                        state.status.connection = Connection::Connected(credential.account.clone());
                        state.emit(AccountEvent::Connected(credential.account));
                    }
                    Err(error) => {
                        state.status.problem = Some(AccountProblem::SignIn(ProviderError::new(
                            self.service,
                            error,
                        )));
                    }
                }
            }
        }
        Some(state.publish(&self.published))
    }

    pub fn cancel_sign_in(&self) -> Versioned<AccountStatus> {
        let mut state = self.state.lock().unwrap();
        state.cancel_attempt(&self.attempts);
        state.publish(&self.published)
    }

    /// Removes the saved sign-in, ends the session and asks the service to revoke it. Blocks on
    /// the credential store and the network.
    ///
    /// If the credential cannot be removed, nothing else changes and the problem says so. A
    /// failed revocation does not restore the local sign-in.
    pub fn sign_out(&self) -> Versioned<AccountStatus> {
        let (flow, saved, signed_out) = {
            let mut state = self.state.lock().unwrap();
            state.cancel_attempt(&self.attempts);
            let saved = self.store.load(self.service);
            if let Err(error) = self.store.delete(self.service) {
                state.status.problem = Some(AccountProblem::Remove(ProviderError::new(
                    self.service,
                    error,
                )));
                return state.publish(&self.published);
            }
            self.session.end();
            state.status.connection = Connection::SignedOut;
            state.status.problem = None;
            state.emit(AccountEvent::SignedOut);
            let signed_out = state.publish(&self.published);
            let Some(flow) = &self.flow else {
                return signed_out;
            };
            (flow, saved, signed_out)
        };

        let revoked = match saved {
            Ok(Some(credential)) => flow.revoke(&credential),
            Ok(None) => Ok(()),
            // The secret was removed, but it could not be read to revoke it.
            Err(error) => Err(ProviderError::new(self.service, error)),
        };
        let Err(error) = revoked else {
            return signed_out;
        };
        let mut state = self.state.lock().unwrap();
        // A newer action owns the status now; its problem is not this revocation's.
        if self.status().version != signed_out.version {
            return self.status();
        }
        state.status.problem = Some(AccountProblem::Revoke(error));
        state.publish(&self.published)
    }

    /// Marks the account expired when a request of the current session finds the sign-in
    /// rejected. Errors from earlier sessions and other errors change nothing.
    pub fn report(
        &self,
        request: &Request,
        error: &ProviderError,
    ) -> Option<Versioned<AccountStatus>> {
        if !error.ends_session() {
            return None;
        }
        let mut state = self.state.lock().unwrap();
        if request.service() != self.service || !self.session.is_current(request.epoch()) {
            return None;
        }
        let Connection::Connected(account) = &state.status.connection else {
            return None;
        };
        let account = account.clone();
        state.status.connection = Connection::Expired(account.clone());
        state.emit(AccountEvent::Expired(account));
        Some(state.publish(&self.published))
    }
}

/// Future returned by [`Account::changed`].
#[derive(Debug)]
pub struct Changed<'a> {
    account: &'a Account,
    seen: SnapshotVersion,
}

impl Future for Changed<'_> {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        let mut published = self.account.published.lock().unwrap();
        if published.snapshot.version > self.seen {
            return Poll::Ready(());
        }
        if !published
            .watchers
            .iter()
            .any(|watcher| watcher.will_wake(cx.waker()))
        {
            published.watchers.push(cx.waker().clone());
        }
        Poll::Pending
    }
}
