//! Cancellable, generation-tagged provider requests.
//!
//! A [`RequestScope`] is one stream of superseding requests, such as searches as the user types.
//! Beginning a request cancels the previous one, and [`RequestScope::accept`] drops any result
//! that is no longer current. Every scope belongs to an account session: signing out or
//! reconnecting cancels the session's requests and drops their late results, so a response
//! fetched with the old account never reaches the new one.

use std::sync::{Arc, Mutex};

use parabolica_domain::control::{CancelToken, Generation, GenerationCounter, Tagged};

use crate::error::{ProviderError, ProviderErrorKind};
use crate::service::ServiceId;

/// The lifetime of one signed-in session. Owned by [`crate::account::Account`].
#[derive(Debug, Default)]
pub(crate) struct SessionGate {
    epochs: GenerationCounter,
    cancel: Mutex<CancelToken>,
}

impl SessionGate {
    fn current(&self) -> (Generation, CancelToken) {
        let cancel = self.cancel.lock().unwrap();
        (self.epochs.current(), cancel.clone())
    }

    pub(crate) fn is_current(&self, epoch: Generation) -> bool {
        self.epochs.is_current(epoch)
    }

    /// Cancels every request made in the session and starts a new one.
    pub(crate) fn end(&self) {
        let mut cancel = self.cancel.lock().unwrap();
        cancel.cancel();
        *cancel = CancelToken::new();
        self.epochs.advance();
    }
}

#[derive(Debug)]
pub struct RequestScope {
    service: ServiceId,
    session: Arc<SessionGate>,
    generations: GenerationCounter,
    active: Mutex<Active>,
}

#[derive(Debug, Default)]
struct Active {
    epoch: Option<Generation>,
    cancel: CancelToken,
}

impl RequestScope {
    pub(crate) fn new(service: ServiceId, session: Arc<SessionGate>) -> Self {
        Self {
            service,
            session,
            generations: GenerationCounter::default(),
            active: Mutex::default(),
        }
    }

    /// Starts a request, cancelling the one before it.
    pub fn begin(&self) -> Request {
        let mut active = self.active.lock().unwrap();
        active.cancel.cancel();
        let (epoch, session) = self.session.current();
        let cancel = CancelToken::new();
        *active = Active {
            epoch: Some(epoch),
            cancel: cancel.clone(),
        };
        Request {
            service: self.service,
            generation: self.generations.advance(),
            epoch,
            cancel,
            session,
        }
    }

    /// Cancels the current request; its result will not be accepted.
    pub fn cancel(&self) {
        let active = self.active.lock().unwrap();
        active.cancel.cancel();
        self.generations.advance();
    }

    /// The result's value if it belongs to the latest request of a session that is still active.
    pub fn accept<T>(&self, result: Tagged<T>) -> Option<T> {
        let active = self.active.lock().unwrap();
        let session_current = active
            .epoch
            .is_some_and(|epoch| self.session.is_current(epoch));
        if session_current {
            result.accept(&self.generations)
        } else {
            None
        }
    }
}

/// One request's identity and cancellation. Adapters poll [`Request::check`] between network
/// calls and return results through [`Request::tag`].
#[derive(Debug, Clone)]
pub struct Request {
    service: ServiceId,
    generation: Generation,
    epoch: Generation,
    cancel: CancelToken,
    session: CancelToken,
}

impl Request {
    pub fn service(&self) -> ServiceId {
        self.service
    }

    pub fn generation(&self) -> Generation {
        self.generation
    }

    pub(crate) fn epoch(&self) -> Generation {
        self.epoch
    }

    /// Cancelled directly, superseded by a newer request, or its session ended.
    pub fn is_cancelled(&self) -> bool {
        self.cancel.is_cancelled() || self.session.is_cancelled()
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

    pub fn tag<T>(&self, value: T) -> Tagged<T> {
        Tagged {
            generation: self.generation,
            value,
        }
    }
}
