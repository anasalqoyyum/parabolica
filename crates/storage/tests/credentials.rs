//! Credentials stay out of the database, its exports and storage error messages.

mod common;

use std::fs;

use common::youtube;
use parabolica_domain::source::PlayableSource;
use parabolica_storage::preferences::Preference;
use parabolica_storage::recovery::export_copy;
use parabolica_storage::sources::LinkState;
use parabolica_storage::{Storage, StorageError};

const CREDENTIAL_WORDS: [&str; 8] = [
    "token",
    "secret",
    "password",
    "passwd",
    "cookie",
    "credential",
    "apikey",
    "api_key",
];

/// Every migration, including ones added by later issues, is checked here: no table or column may
/// be named for a credential.
#[test]
fn no_table_or_column_is_named_for_a_credential() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("parabolica.db");
    drop(Storage::open(&path).unwrap());

    let conn =
        rusqlite::Connection::open_with_flags(&path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .unwrap();
    let mut statement = conn
        .prepare(
            "SELECT m.name, p.name FROM sqlite_schema m, pragma_table_info(m.name) p
             WHERE m.type = 'table'",
        )
        .unwrap();
    let columns: Vec<(String, String)> = statement
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert!(columns.iter().any(|(table, _)| table == "preferences"));
    for (table, column) in columns {
        let lower = format!("{table}.{column}").to_ascii_lowercase();
        assert!(
            !CREDENTIAL_WORDS.iter().any(|word| lower.contains(word)),
            "{table}.{column} looks like a credential; credentials belong in OS credential storage"
        );
    }
}

struct SessionToken(String);

impl Preference for SessionToken {
    const KEY: &'static str = "youtube.session_token";

    fn encode(&self) -> String {
        self.0.clone()
    }

    fn decode(stored: &str) -> Option<Self> {
        Some(SessionToken(stored.to_owned()))
    }
}

#[test]
fn a_credential_preference_is_refused_and_never_reaches_the_file_or_its_export() {
    const SECRET: &str = "ya29.a0-not-a-real-token-5f1e";
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("parabolica.db");
    let storage = Storage::open(&path).unwrap();

    let error = storage
        .save_preference(&SessionToken(SECRET.to_owned()))
        .unwrap_err();
    assert!(matches!(error, StorageError::Invalid { .. }));
    assert!(!error.to_string().contains(SECRET));
    assert!(!format!("{error:?}").contains(SECRET));

    // Provider identity is stored; authentication material is not part of the contract.
    let track = storage.create_track().unwrap();
    storage
        .link_source(
            track,
            PlayableSource::Provider(youtube("dQw4w9WgXcQ")),
            LinkState::Confirmed,
            None,
        )
        .unwrap();
    drop(storage);

    let export = export_copy(&path, &dir.path().join("export")).unwrap();
    for file in export
        .iter()
        .map(|path| path.as_path())
        .chain([path.as_path()])
    {
        let bytes = fs::read(file).unwrap();
        assert!(
            !bytes
                .windows(SECRET.len())
                .any(|window| window == SECRET.as_bytes()),
            "{} contains the secret",
            file.display()
        );
    }
}

#[test]
fn constraint_failures_name_the_rule_without_echoing_values() {
    const PRIVATE: &str = "C:/Users/someone/Private Folder";
    let dir = tempfile::tempdir().unwrap();
    let storage = Storage::open(dir.path().join("parabolica.db")).unwrap();
    let missing_track = parabolica_domain::ids::TrackId::generate();

    let error = storage
        .link_source(
            missing_track,
            PlayableSource::Provider(youtube(PRIVATE)),
            LinkState::Suggested,
            Some(PRIVATE.to_owned()),
        )
        .unwrap_err();
    assert!(
        error.to_string().contains("FOREIGN KEY constraint failed"),
        "{error}"
    );
    assert!(!error.to_string().contains(PRIVATE));
    assert!(!format!("{error:?}").contains(PRIVATE));
}
