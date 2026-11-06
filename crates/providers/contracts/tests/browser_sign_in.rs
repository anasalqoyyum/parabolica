//! Browser sign-in through the loopback receiver, with a simulated browser that follows the
//! authorization URL's redirect. Codes and secrets are disposable test strings.

use std::borrow::Cow;
use std::io::{Read as _, Write as _};
use std::net::TcpStream;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use parabolica_provider_contracts::account::{Account, AccountProblem, Connection};
use parabolica_provider_contracts::auth::loopback::LoopbackReceiver;
use parabolica_provider_contracts::auth::{
    AuthAttempt, AuthFlow, AuthorizationError, Browser, BrowserAuthorization, SignedIn,
};
use parabolica_provider_contracts::credentials::{
    CredentialStore, MemoryCredentialStore, StoredCredential,
};
use parabolica_provider_contracts::error::{ProviderError, ProviderErrorKind};
use parabolica_provider_contracts::service::{AccountIdentity, ServiceId};
use sha2::{Digest as _, Sha256};
use url::Url;

const SERVICE: ServiceId = ServiceId::YOUTUBE_MUSIC;
const CODE: &str = "disposable-code-7f3a";

fn query<'a>(
    pairs: &'a [(&'a str, &'a str)],
) -> impl Iterator<Item = (Cow<'a, str>, Cow<'a, str>)> {
    pairs
        .iter()
        .map(|(key, value)| (Cow::Borrowed(*key), Cow::Borrowed(*value)))
}

#[test]
fn state_and_pkce_values_are_random_and_s256() {
    let a = BrowserAuthorization::new().unwrap();
    let b = BrowserAuthorization::new().unwrap();
    assert_ne!(a.state(), b.state());
    assert_eq!(a.state().len(), 43);
    let verifier = a.code_verifier().expose_str().unwrap();
    assert_eq!(verifier.len(), 43);
    assert_eq!(
        a.code_challenge(),
        URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
    );

    let mut url = Url::parse("https://accounts.example.test/authorize?client_id=app").unwrap();
    a.apply(&mut url);
    let pairs: Vec<(String, String)> = url.query_pairs().into_owned().collect();
    assert!(pairs.contains(&("state".to_owned(), a.state().to_owned())));
    assert!(pairs.contains(&("code_challenge".to_owned(), a.code_challenge())));
    assert!(pairs.contains(&("code_challenge_method".to_owned(), "S256".to_owned())));
    assert!(
        !url.as_str().contains(verifier),
        "the verifier stays in the app"
    );
}

#[test]
fn callbacks_are_validated_against_the_request_state() {
    let authorization = BrowserAuthorization::new().unwrap();
    let state = authorization.state().to_owned();

    let code = authorization
        .validate(query(&[("code", CODE), ("state", &state)]))
        .unwrap();
    assert_eq!(code.expose(), CODE.as_bytes());

    for pairs in [
        vec![("code", CODE)],
        vec![("code", CODE), ("state", "another-request")],
        vec![("code", CODE), ("state", &state), ("state", &state)],
        vec![("code", CODE), ("state", &state[..42])],
    ] {
        assert_eq!(
            authorization.validate(query(&pairs)).unwrap_err(),
            AuthorizationError::StateMismatch,
            "{pairs:?}"
        );
    }
    assert_eq!(
        authorization
            .validate(query(&[("error", "access_denied"), ("state", &state)]))
            .unwrap_err(),
        AuthorizationError::Denied
    );
    assert_eq!(
        authorization
            .validate(query(&[("code", ""), ("state", &state)]))
            .unwrap_err(),
        AuthorizationError::MissingCode
    );
}

/// What the simulated browser does after the user approves.
#[derive(Debug, Clone, Copy, PartialEq)]
enum UserAction {
    Approve,
    Decline,
    /// Leaves the tab open; nothing reaches the redirect.
    Ignore,
}

/// Follows the authorization URL's redirect. Before the real redirect it sends a request for
/// another path and one carrying a stale state, as a leftover tab would.
#[derive(Debug)]
struct SimulatedBrowser {
    action: UserAction,
    responses: Arc<Mutex<Vec<String>>>,
    visit: Mutex<Option<thread::JoinHandle<()>>>,
}

impl SimulatedBrowser {
    fn new(action: UserAction) -> Self {
        Self {
            action,
            responses: Arc::default(),
            visit: Mutex::default(),
        }
    }

    /// Waits for every request to finish and returns the responses.
    fn responses(&self) -> Vec<String> {
        if let Some(visit) = self.visit.lock().unwrap().take() {
            visit.join().unwrap();
        }
        self.responses.lock().unwrap().clone()
    }
}

impl Browser for SimulatedBrowser {
    fn open(&self, url: &Url) -> Result<(), AuthorizationError> {
        let pairs: Vec<(String, String)> = url.query_pairs().into_owned().collect();
        let get = |key: &str| pairs.iter().find(|(k, _)| k == key).unwrap().1.clone();
        let redirect = Url::parse(&get("redirect_uri")).unwrap();
        let state = get("state");
        let action = self.action;
        let responses = self.responses.clone();
        let visit = thread::spawn(move || {
            let send = |target: String| {
                let response = http_get(&redirect, &target);
                responses.lock().unwrap().push(response);
            };
            send("/favicon.ico".to_owned());
            send(format!(
                "{}?code=stale-code&state=old-state",
                redirect.path()
            ));
            match action {
                UserAction::Approve => send(format!(
                    "{}?code={CODE}&state={state}&scope=read",
                    redirect.path()
                )),
                UserAction::Decline => send(format!(
                    "{}?error=access_denied&state={state}",
                    redirect.path()
                )),
                UserAction::Ignore => {}
            }
        });
        *self.visit.lock().unwrap() = Some(visit);
        Ok(())
    }
}

fn http_get(base: &Url, target: &str) -> String {
    let mut stream = TcpStream::connect((base.host_str().unwrap(), base.port().unwrap())).unwrap();
    write!(
        stream,
        "GET {target} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n"
    )
    .unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    response
}

/// A provider flow built from the helpers. The token exchange is simulated: the code becomes
/// the secret.
#[derive(Debug)]
struct LoopbackFlow {
    timeout: Duration,
    account_id: &'static str,
}

impl AuthFlow for LoopbackFlow {
    fn sign_in(
        &self,
        attempt: &AuthAttempt,
        browser: &dyn Browser,
    ) -> Result<SignedIn, ProviderError> {
        let receiver = LoopbackReceiver::bind("/callback").map_err(|error| attempt.fail(error))?;
        let authorization = BrowserAuthorization::new().map_err(|error| attempt.fail(error))?;
        let mut url = Url::parse("https://accounts.example.test/authorize").unwrap();
        url.query_pairs_mut()
            .append_pair("client_id", "parabolica-test")
            .append_pair("redirect_uri", receiver.redirect_uri().as_str());
        authorization.apply(&mut url);
        browser.open(&url).map_err(|error| attempt.fail(error))?;
        let code = receiver.receive(&authorization, attempt, self.timeout)?;
        Ok(SignedIn {
            account: AccountIdentity {
                id: self.account_id.to_owned(),
                display_name: "Test account".to_owned(),
            },
            secret: code,
        })
    }

    fn revoke(&self, _: &StoredCredential) -> Result<(), ProviderError> {
        Ok(())
    }
}

fn account(flow: LoopbackFlow) -> (Account, Arc<MemoryCredentialStore>) {
    let store = Arc::new(MemoryCredentialStore::new());
    (
        Account::new(SERVICE, store.clone(), Some(Arc::new(flow))),
        store,
    )
}

#[test]
fn an_approved_redirect_signs_in_and_stale_requests_are_ignored() {
    let (account, store) = account(LoopbackFlow {
        timeout: Duration::from_secs(10),
        account_id: "a",
    });
    let browser = SimulatedBrowser::new(UserAction::Approve);
    let (attempt, _) = account.begin_sign_in().unwrap();

    let status = account.sign_in(&attempt, &browser).unwrap().state;
    assert!(
        matches!(status.connection, Connection::Connected(_)),
        "{status:?}"
    );
    let saved = store.load(SERVICE).unwrap().unwrap();
    assert_eq!(saved.secret.expose(), CODE.as_bytes());

    let responses = browser.responses();
    assert!(responses[0].starts_with("HTTP/1.1 404"));
    assert!(responses[1].starts_with("HTTP/1.1 400"));
    assert!(responses[2].starts_with("HTTP/1.1 200"));
    assert!(responses[2].contains("Cache-Control: no-store"));
    assert!(
        !responses[2].contains(CODE),
        "the page does not echo the code"
    );
}

#[test]
fn a_declined_redirect_fails_without_saving() {
    let (account, store) = account(LoopbackFlow {
        timeout: Duration::from_secs(10),
        account_id: "a",
    });
    let (attempt, _) = account.begin_sign_in().unwrap();
    let status = account
        .sign_in(&attempt, &SimulatedBrowser::new(UserAction::Decline))
        .unwrap()
        .state;
    assert_eq!(status.connection, Connection::SignedOut);
    assert_eq!(
        status.problem,
        Some(AccountProblem::SignIn(ProviderError::new(
            SERVICE,
            AuthorizationError::Denied
        )))
    );
    assert!(!store.contains(SERVICE));
}

#[test]
fn an_unanswered_sign_in_times_out() {
    let (account, _) = account(LoopbackFlow {
        timeout: Duration::from_millis(300),
        account_id: "a",
    });
    let (attempt, _) = account.begin_sign_in().unwrap();
    let status = account
        .sign_in(&attempt, &SimulatedBrowser::new(UserAction::Ignore))
        .unwrap()
        .state;
    assert_eq!(
        status.problem,
        Some(AccountProblem::SignIn(ProviderError::new(
            SERVICE,
            AuthorizationError::TimedOut
        )))
    );
}

#[test]
fn cancelling_stops_the_waiting_receiver() {
    let (account, store) = account(LoopbackFlow {
        timeout: Duration::from_secs(30),
        account_id: "a",
    });
    let account = Arc::new(account);
    let (attempt, _) = account.begin_sign_in().unwrap();
    let started = Instant::now();
    let worker = {
        let account = account.clone();
        let attempt = attempt.clone();
        thread::spawn(move || account.sign_in(&attempt, &SimulatedBrowser::new(UserAction::Ignore)))
    };
    thread::sleep(Duration::from_millis(100));
    let status = account.cancel_sign_in().state;
    assert!(!status.signing_in);

    // The superseded attempt's result is not applied.
    assert_eq!(worker.join().unwrap(), None);
    assert!(started.elapsed() < Duration::from_secs(5));
    assert!(!store.contains(SERVICE));
}

#[test]
fn a_receiver_reports_cancellation_as_cancelled() {
    let (account, _) = account(LoopbackFlow {
        timeout: Duration::from_secs(30),
        account_id: "a",
    });
    let (attempt, _) = account.begin_sign_in().unwrap();
    account.cancel_sign_in();
    let receiver = LoopbackReceiver::bind("/callback").unwrap();
    let authorization = BrowserAuthorization::new().unwrap();
    let error = receiver
        .receive(&authorization, &attempt, Duration::from_secs(30))
        .unwrap_err();
    assert_eq!(error.kind, ProviderErrorKind::Cancelled);
}

#[test]
fn the_redirect_uri_is_loopback_on_an_ephemeral_port() {
    let receiver = LoopbackReceiver::bind("/callback").unwrap();
    let uri = receiver.redirect_uri();
    assert_eq!(uri.scheme(), "http");
    assert_eq!(uri.host_str(), Some("127.0.0.1"));
    assert_ne!(uri.port(), None);
    assert_eq!(uri.path(), "/callback");
}

#[test]
fn a_slow_client_cannot_hold_the_receiver_past_cancellation() {
    let account = Arc::new(
        account(LoopbackFlow {
            timeout: Duration::from_secs(30),
            account_id: "a",
        })
        .0,
    );
    let (attempt, _) = account.begin_sign_in().unwrap();
    let receiver = LoopbackReceiver::bind("/callback").unwrap();
    let authorization = BrowserAuthorization::new().unwrap();
    let port = receiver.redirect_uri().port().unwrap();
    let dripping = thread::spawn(move || {
        let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        for byte in b"GET /callback?state=".iter().cycle().take(40) {
            if stream.write_all(&[*byte]).is_err() {
                break;
            }
            thread::sleep(Duration::from_millis(200));
        }
    });
    let started = Instant::now();
    let cancel = {
        let account = account.clone();
        thread::spawn(move || {
            thread::sleep(Duration::from_millis(300));
            account.cancel_sign_in();
        })
    };
    let error = receiver
        .receive(&authorization, &attempt, Duration::from_secs(30))
        .unwrap_err();
    assert_eq!(error.kind, ProviderErrorKind::Cancelled);
    assert!(
        started.elapsed() < Duration::from_secs(4),
        "{:?}",
        started.elapsed()
    );
    cancel.join().unwrap();
    dripping.join().unwrap();
}

#[test]
fn a_scheme_relative_target_is_not_the_redirect() {
    let (account, _) = account(LoopbackFlow {
        timeout: Duration::from_secs(30),
        account_id: "a",
    });
    let (attempt, _) = account.begin_sign_in().unwrap();
    let receiver = LoopbackReceiver::bind("/callback").unwrap();
    let authorization = BrowserAuthorization::new().unwrap();
    let base = receiver.redirect_uri().clone();
    let state = authorization.state().to_owned();
    let visit = thread::spawn(move || {
        let foreign = http_get(
            &base,
            &format!("//evil.example/callback?code={CODE}&state={state}"),
        );
        let real = http_get(&base, &format!("/callback?code={CODE}&state={state}"));
        (foreign, real)
    });
    let code = receiver
        .receive(&authorization, &attempt, Duration::from_secs(10))
        .unwrap();
    assert_eq!(code.expose(), CODE.as_bytes());
    let (foreign, real) = visit.join().unwrap();
    assert!(foreign.starts_with("HTTP/1.1 404"), "{foreign}");
    assert!(real.starts_with("HTTP/1.1 200"));
}
