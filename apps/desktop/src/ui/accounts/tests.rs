//! Account cards in the real shell, with an in-memory credential store and a scripted sign-in
//! flow. Secrets are disposable test strings.

use std::sync::{Arc, Mutex};

use gpui_kit::test::TestWindowExt as _;
use gpui_kit::{
    AnyWindowHandle, AppContext as _, Bounds, Point, TestAppContext, WindowBounds, WindowOptions,
    px, size,
};
use parabolica_domain::provider::UnavailableReason;
use parabolica_provider_contracts::account::{Account, Connection};
use parabolica_provider_contracts::auth::{
    AuthAttempt, AuthFlow, AuthorizationError, Browser, SignedIn, Url,
};
use parabolica_provider_contracts::credentials::{
    CredentialError, CredentialStore, MemoryCredentialStore, StoredCredential,
};
use parabolica_provider_contracts::error::ProviderError;
use parabolica_provider_contracts::secret::Secret;
use parabolica_provider_contracts::service::{AccountIdentity, ServiceId};

use crate::ui::appearance;
use crate::ui::shell::{self, Page, SettingsSection, Shell};

const SERVICE: ServiceId = ServiceId::YOUTUBE_MUSIC;
const STATUS: &str = "account-youtube-music-status";
const CONNECT: &str = "account-youtube-music-connect";
const DISCONNECT: &str = "account-youtube-music-disconnect";
const PROBLEM: &str = "account-youtube-music-problem";
const UNAVAILABLE: &str = "account-youtube-music-sign-in-unavailable";

fn identity(id: &str) -> AccountIdentity {
    AccountIdentity {
        id: id.to_owned(),
        display_name: format!("Account {id}"),
    }
}

#[derive(Debug, Default)]
struct ScriptedFlow {
    results: Mutex<Vec<Result<SignedIn, ProviderError>>>,
}

impl ScriptedFlow {
    fn then_sign_in_as(&self, id: &str) {
        self.results.lock().unwrap().push(Ok(SignedIn {
            account: identity(id),
            secret: Secret::new(format!("disposable-secret-{id}")),
        }));
    }
}

impl AuthFlow for ScriptedFlow {
    fn sign_in(&self, _: &AuthAttempt, _: &dyn Browser) -> Result<SignedIn, ProviderError> {
        self.results.lock().unwrap().remove(0)
    }

    fn revoke(&self, _: &StoredCredential) -> Result<(), ProviderError> {
        Ok(())
    }
}

#[derive(Debug)]
struct NoBrowser;

impl Browser for NoBrowser {
    fn open(&self, _: &Url) -> Result<(), AuthorizationError> {
        Ok(())
    }
}

fn open(cx: &mut TestAppContext, account: Arc<Account>) -> AnyWindowHandle {
    let window = cx.update(|cx| {
        gpui_kit::init(cx);
        shell::init(cx);
        appearance::init(None, None, cx);
        super::init_with(vec![account], Arc::new(NoBrowser), cx);
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds {
                origin: Point::default(),
                size: size(px(1280.), px(800.)),
            })),
            ..Default::default()
        };
        gpui_kit::open_window(options, cx, |window, cx| {
            cx.new(|cx| {
                let mut shell = Shell::new(None, window, cx);
                shell.navigate(Page::Settings(SettingsSection::SourcesAndOutput), cx);
                shell
            })
        })
        .expect("open the shell window")
        .0
    });
    cx.run_until_parked();
    window
}

fn status(cx: &mut TestAppContext, window: AnyWindowHandle) -> String {
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.find(STATUS).value().unwrap_or_default().to_owned()
    })
    .unwrap()
}

fn click(cx: &mut TestAppContext, window: AnyWindowHandle, id: &'static str) {
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click(id, cx);
    })
    .unwrap();
    cx.run_until_parked();
}

fn saved(store: &MemoryCredentialStore) -> Option<String> {
    store
        .load(SERVICE)
        .unwrap()
        .map(|credential| String::from_utf8(credential.secret.expose().to_vec()).unwrap())
}

#[gpui_kit::test]
fn a_restored_sign_in_is_shown_and_disconnect_removes_it(cx: &mut TestAppContext) {
    let store = Arc::new(MemoryCredentialStore::new());
    store
        .save(
            SERVICE,
            &StoredCredential {
                account: identity("a"),
                secret: Secret::new("disposable-saved-secret"),
            },
        )
        .unwrap();
    let account = Arc::new(Account::new(SERVICE, store.clone(), None));
    let window = open(cx, account);

    assert_eq!(status(cx, window), "Account a · Connected");
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find(CONNECT).is_none());
        assert!(window.try_find(UNAVAILABLE).is_some());
    })
    .unwrap();

    click(cx, window, DISCONNECT);
    assert_eq!(saved(&store), None);
    assert_eq!(status(cx, window), "Not connected");
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(window.find(CONNECT).disabled(), Some(true));
        assert!(window.try_find(DISCONNECT).is_none());
        assert!(window.try_find(PROBLEM).is_none());
    })
    .unwrap();
}

#[gpui_kit::test]
fn a_secret_that_cannot_be_removed_keeps_the_account_connected(cx: &mut TestAppContext) {
    let store = Arc::new(MemoryCredentialStore::new());
    store
        .save(
            SERVICE,
            &StoredCredential {
                account: identity("a"),
                secret: Secret::new("disposable-saved-secret"),
            },
        )
        .unwrap();
    let window = open(cx, Arc::new(Account::new(SERVICE, store.clone(), None)));
    store.fail_with(Some(CredentialError::Os {
        operation: "remove",
        code: 0x8007_0005,
    }));

    click(cx, window, DISCONNECT);
    assert_eq!(status(cx, window), "Account a · Connected");
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(
            window.find(PROBLEM).label(),
            Some(
                "Still connected. The saved sign-in could not be removed: the system credential \
                 store could not remove the sign-in (error 0x80070005)."
            )
        );
    })
    .unwrap();
}

#[gpui_kit::test]
fn connect_expire_and_reconnect_only_the_same_account(cx: &mut TestAppContext) {
    let store = Arc::new(MemoryCredentialStore::new());
    let flow = Arc::new(ScriptedFlow::default());
    let account = Arc::new(Account::new(SERVICE, store.clone(), Some(flow.clone())));
    let window = open(cx, account.clone());
    assert_eq!(status(cx, window), "Not connected");
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert_ne!(window.find(CONNECT).disabled(), Some(true));
        assert_eq!(window.find(CONNECT).label(), Some("Connect"));
        assert!(window.try_find(UNAVAILABLE).is_none());
    })
    .unwrap();

    flow.then_sign_in_as("a");
    click(cx, window, CONNECT);
    assert_eq!(status(cx, window), "Account a · Connected");
    assert_eq!(saved(&store).as_deref(), Some("disposable-secret-a"));

    // A provider controller finds the session rejected; the card follows without a UI action.
    let request = account.requests().begin();
    account.report(
        &request,
        &ProviderError::unavailable(SERVICE, UnavailableReason::SessionExpired),
    );
    cx.run_until_parked();
    assert_eq!(status(cx, window), "Account a · Session expired");
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(window.find(CONNECT).label(), Some("Reconnect"));
        assert!(window.try_find(DISCONNECT).is_some());
    })
    .unwrap();

    flow.then_sign_in_as("someone-else");
    click(cx, window, CONNECT);
    assert_eq!(status(cx, window), "Account a · Session expired");
    assert_eq!(saved(&store).as_deref(), Some("disposable-secret-a"));
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(
            window.find(PROBLEM).label(),
            Some("Sign-in used a different account. Disconnect first to switch accounts.")
        );
    })
    .unwrap();

    flow.then_sign_in_as("a");
    click(cx, window, CONNECT);
    assert_eq!(status(cx, window), "Account a · Connected");
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find(PROBLEM).is_none());
    })
    .unwrap();
    assert!(matches!(
        account.status().state.connection,
        Connection::Connected(_)
    ));
}
