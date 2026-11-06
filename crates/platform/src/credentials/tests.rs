//! Round trips through the real Windows Credential Manager. Each test uses its own namespace
//! with disposable secrets and removes what it wrote.

use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use parabolica_provider_contracts::credentials::{
    CredentialError, CredentialStore, StoredCredential,
};
use parabolica_provider_contracts::secret::Secret;
use parabolica_provider_contracts::service::{AccountIdentity, ServiceId};

use super::OsCredentialStore;

const NAMESPACE_VAR: &str = "PARABOLICA_CREDENTIAL_TEST_NAMESPACE";
const EXPECT_VAR: &str = "PARABOLICA_CREDENTIAL_TEST_EXPECT";
const SERVICES: [ServiceId; 3] = [
    ServiceId::YOUTUBE_MUSIC,
    ServiceId::DISCOGS,
    ServiceId::Scrobbling,
];

/// A store in a namespace no other test or app instance uses. Dropping it removes every
/// credential it may have written.
struct Scratch(OsCredentialStore);

impl Scratch {
    fn new(test: &str) -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        Self(OsCredentialStore::with_namespace(&format!(
            "Parabolica-test-{test}-{}-{nanos}",
            std::process::id()
        )))
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        for service in SERVICES {
            let _ = self.0.delete(service);
        }
    }
}

fn credential(secret: &[u8]) -> StoredCredential {
    StoredCredential {
        account: AccountIdentity {
            id: "account-1@example.test".to_owned(),
            display_name: "Test account ✓ テスト".to_owned(),
        },
        secret: Secret::new(secret),
    }
}

#[test]
fn a_credential_round_trips_and_is_removed() {
    let store = Scratch::new("round-trip");
    let service = ServiceId::YOUTUBE_MUSIC;
    assert!(store.0.load(service).unwrap().is_none());

    let secret = b"disposable-secret\0with binary \xff bytes";
    store.0.save(service, &credential(secret)).unwrap();
    let loaded = store.0.load(service).unwrap().unwrap();
    assert_eq!(loaded.account, credential(secret).account);
    assert_eq!(loaded.secret.expose(), secret);

    store.0.save(service, &credential(b"replacement")).unwrap();
    assert_eq!(
        store.0.load(service).unwrap().unwrap().secret.expose(),
        b"replacement"
    );
    assert!(store.0.load(ServiceId::DISCOGS).unwrap().is_none());

    store.0.delete(service).unwrap();
    assert!(store.0.load(service).unwrap().is_none());
    store.0.delete(service).unwrap();
}

#[test]
fn an_oversized_secret_is_refused_without_writing() {
    let store = Scratch::new("oversized");
    let error = store
        .0
        .save(ServiceId::Scrobbling, &credential(&[7u8; 2561]))
        .unwrap_err();
    assert_eq!(
        error,
        CredentialError::TooLarge {
            field: "secret",
            len: 2561,
            max: 2560
        }
    );
    assert!(store.0.load(ServiceId::Scrobbling).unwrap().is_none());

    store
        .0
        .save(ServiceId::Scrobbling, &credential(&[7u8; 2560]))
        .unwrap();
    assert_eq!(
        store
            .0
            .load(ServiceId::Scrobbling)
            .unwrap()
            .unwrap()
            .secret
            .len(),
        2560
    );
}

/// Writes in this process, then reads and removes from fresh processes, as an app restart would.
#[test]
fn a_credential_persists_across_processes_until_removed() {
    let store = Scratch::new("restart");
    let namespace = store.0.namespace.clone();
    store
        .0
        .save(
            ServiceId::YOUTUBE_MUSIC,
            &credential(b"disposable-restart-secret"),
        )
        .unwrap();

    run_child(&namespace, "present");
    store.0.delete(ServiceId::YOUTUBE_MUSIC).unwrap();
    run_child(&namespace, "absent");
}

fn run_child(namespace: &str, expect: &str) {
    let output = Command::new(std::env::current_exe().unwrap())
        .args([
            "credentials::tests::child_process_check",
            "--exact",
            "--ignored",
            "--nocapture",
            "--test-threads=1",
        ])
        .env(NAMESPACE_VAR, namespace)
        .env(EXPECT_VAR, expect)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success() && stdout.contains("1 passed"),
        "child expecting {expect} failed:\n{stdout}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
#[ignore = "run by a_credential_persists_across_processes_until_removed in a child process"]
fn child_process_check() {
    let Ok(namespace) = std::env::var(NAMESPACE_VAR) else {
        return;
    };
    let store = OsCredentialStore::with_namespace(&namespace);
    let loaded = store.load(ServiceId::YOUTUBE_MUSIC).unwrap();
    match std::env::var(EXPECT_VAR).unwrap().as_str() {
        "present" => {
            let loaded = loaded.unwrap();
            assert_eq!(loaded.secret.expose(), b"disposable-restart-secret");
            assert_eq!(loaded.account.display_name, "Test account ✓ テスト");
        }
        "absent" => assert!(loaded.is_none()),
        other => panic!("unknown expectation {other}"),
    }
}
