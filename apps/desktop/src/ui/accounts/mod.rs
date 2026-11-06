//! Provider accounts: restoring saved sign-ins, connecting, reconnecting and disconnecting.
//!
//! Each service is a contracts [`Account`] backed by the OS credential store. A provider enables
//! sign-in by passing its `AuthFlow` to its `Account` in [`init`]; until then the card can show
//! and remove a saved sign-in but not create one. Controllers that hold a provider session get
//! the account through [`account`], subscribe to its lifecycle events and make requests through
//! its request scopes. Account work runs off the UI thread. One task per account awaits its
//! status changes, whoever made them, and applies them in snapshot order.

mod page;
pub mod scrobbling;
#[cfg(test)]
mod tests;

use std::sync::Arc;

use gpui_kit::{App, Global};
use parabolica_domain::control::{SnapshotGate, SnapshotVersion, Versioned};
use parabolica_platform::credentials::{OsCredentialStore, SystemBrowser};
use parabolica_provider_contracts::account::{Account, AccountStatus};
use parabolica_provider_contracts::auth::Browser;
use parabolica_provider_contracts::credentials::CredentialStore;
use parabolica_provider_contracts::service::ServiceId;

pub use page::AccountsSettings;

struct Accounts {
    entries: Vec<Entry>,
    browser: Arc<dyn Browser>,
}

impl Global for Accounts {}

struct Entry {
    account: Arc<Account>,
    status: AccountStatus,
    gate: SnapshotGate,
    /// A sign-in task is running; set on click, before its first status arrives.
    connecting: bool,
    disconnecting: bool,
}

/// What a card shows for one service.
#[derive(Debug, Clone)]
struct AccountView {
    status: AccountStatus,
    can_sign_in: bool,
    connecting: bool,
    disconnecting: bool,
}

/// Registers the app's accounts and restores their saved sign-ins in the background.
pub fn init(cx: &mut App) {
    let store: Arc<dyn CredentialStore> = Arc::new(OsCredentialStore::new());
    let youtube_music = Account::new(ServiceId::YOUTUBE_MUSIC, store, None);
    init_with(vec![Arc::new(youtube_music)], Arc::new(SystemBrowser), cx);
}

fn init_with(accounts: Vec<Arc<Account>>, browser: Arc<dyn Browser>, cx: &mut App) {
    let mut watches = Vec::new();
    let entries = accounts
        .iter()
        .map(|account| {
            let snapshot = account.status();
            watches.push((account.clone(), snapshot.version));
            let mut gate = SnapshotGate::default();
            let status = gate
                .accept(snapshot)
                .expect("the first snapshot is accepted");
            Entry {
                account: account.clone(),
                status,
                gate,
                connecting: false,
                disconnecting: false,
            }
        })
        .collect();
    cx.set_global(Accounts { entries, browser });
    for (account, seen) in watches {
        watch(account.clone(), seen, cx);
        run(account, cx, |account| {
            account.restore();
        });
    }
}

/// Applies every status published after `seen`.
fn watch(account: Arc<Account>, mut seen: SnapshotVersion, cx: &mut App) {
    cx.spawn(async move |cx| {
        loop {
            account.changed(seen).await;
            let snapshot = account.status();
            seen = snapshot.version;
            cx.update(|cx| apply(account.service(), snapshot, cx));
        }
    })
    .detach();
}

/// The account for `service`, for controllers that hold its provider session.
pub fn account(service: ServiceId, cx: &App) -> Option<Arc<Account>> {
    cx.global::<Accounts>()
        .entries
        .iter()
        .find(|entry| entry.account.service() == service)
        .map(|entry| entry.account.clone())
}

fn views(cx: &App) -> Vec<AccountView> {
    cx.global::<Accounts>()
        .entries
        .iter()
        .map(|entry| AccountView {
            status: entry.status.clone(),
            can_sign_in: entry.account.can_sign_in(),
            connecting: entry.connecting,
            disconnecting: entry.disconnecting,
        })
        .collect()
}

fn entry_mut(service: ServiceId, cx: &mut App) -> Option<&mut Entry> {
    cx.global_mut::<Accounts>()
        .entries
        .iter_mut()
        .find(|entry| entry.account.service() == service)
}

fn apply(service: ServiceId, snapshot: Versioned<AccountStatus>, cx: &mut App) {
    if let Some(entry) = entry_mut(service, cx)
        && let Some(status) = entry.gate.accept(snapshot)
    {
        entry.status = status;
    }
    cx.refresh_windows();
}

/// Runs blocking account work in the background. Its status changes arrive through [`watch`].
fn run(account: Arc<Account>, cx: &mut App, work: impl FnOnce(&Account) + Send + 'static) {
    cx.background_executor()
        .spawn(async move { work(&account) })
        .detach();
}

/// Signs in through the system browser, or reconnects the expired account.
pub fn connect(service: ServiceId, cx: &mut App) {
    let browser = cx.global::<Accounts>().browser.clone();
    let Some(entry) = entry_mut(service, cx) else {
        return;
    };
    if entry.connecting {
        return;
    }
    entry.connecting = true;
    let account = entry.account.clone();
    cx.refresh_windows();

    // Blocks a background thread until the browser answers, the user cancels or the provider's
    // flow times out.
    let task = cx.background_executor().spawn(async move {
        if let Some((attempt, _)) = account.begin_sign_in() {
            account.sign_in(&attempt, browser.as_ref());
        }
    });
    cx.spawn(async move |cx| {
        task.await;
        cx.update(|cx| {
            if let Some(entry) = entry_mut(service, cx) {
                entry.connecting = false;
            }
            cx.refresh_windows();
        });
    })
    .detach();
}

pub fn cancel_sign_in(service: ServiceId, cx: &mut App) {
    if let Some(account) = account(service, cx) {
        run(account, cx, |account| {
            account.cancel_sign_in();
        });
    }
}

/// Removes the saved sign-in, stops the provider session and revokes it at the service.
pub fn disconnect(service: ServiceId, cx: &mut App) {
    let Some(entry) = entry_mut(service, cx) else {
        return;
    };
    if entry.disconnecting {
        return;
    }
    entry.disconnecting = true;
    let account = entry.account.clone();
    cx.refresh_windows();

    let task = cx
        .background_executor()
        .spawn(async move { account.sign_out() });
    cx.spawn(async move |cx| {
        let snapshot = task.await;
        cx.update(|cx| {
            if let Some(entry) = entry_mut(service, cx) {
                entry.disconnecting = false;
            }
            apply(service, snapshot, cx);
        });
    })
    .detach();
}
