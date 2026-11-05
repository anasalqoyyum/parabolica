//! Damaged, foreign, newer and in-use databases are reported with recovery options and never
//! overwritten.

mod common;

use std::fs;
use std::io::{Seek, SeekFrom, Write};
use std::path::Path;

use common::{AppearanceMode, file_record};
use parabolica_storage::recovery::{export_copy, move_aside};
use parabolica_storage::{Incompatibility, OpenError, RecoveryOption, Storage};

fn populated(path: &Path) {
    let storage = Storage::open(path).unwrap();
    let root = storage.add_library_root(Path::new("/music"), None).unwrap();
    for index in 0..400 {
        storage
            .record_media_file(file_record(
                root,
                &format!("album/{index:03}.flac"),
                &[
                    ("TITLE", &format!("Track number {index}")),
                    ("ARTIST", "Someone"),
                ],
            ))
            .unwrap();
    }
    storage.save_preference(&AppearanceMode::Light).unwrap();
}

fn assert_recoverable(error: &OpenError) {
    assert_eq!(
        error.recovery_options(),
        [
            RecoveryOption::ExportCopy,
            RecoveryOption::MoveAsideAndStartFresh
        ]
    );
}

#[test]
fn a_file_that_is_not_a_database_is_reported_and_left_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("parabolica.db");
    let garbage = b"this was never an SQLite database, but it is the user's file".repeat(100);
    fs::write(&path, &garbage).unwrap();

    let error = Storage::open(&path).unwrap_err();
    assert!(matches!(error, OpenError::Corrupt { .. }), "{error:?}");
    assert_recoverable(&error);
    assert_eq!(fs::read(&path).unwrap(), garbage);
}

#[test]
fn a_damaged_page_is_detected_and_recovery_keeps_the_original_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("parabolica.db");
    populated(&path);

    // Overwrite the middle of the file, past the header and schema pages.
    let length = fs::metadata(&path).unwrap().len();
    let mut file = fs::OpenOptions::new().write(true).open(&path).unwrap();
    file.seek(SeekFrom::Start(length / 2)).unwrap();
    file.write_all(&[0xA5; 8192]).unwrap();
    drop(file);
    let damaged = fs::read(&path).unwrap();

    let error = Storage::open(&path).unwrap_err();
    assert!(matches!(error, OpenError::Corrupt { .. }), "{error:?}");
    assert_recoverable(&error);
    assert_eq!(fs::read(&path).unwrap(), damaged);

    let exported = export_copy(&path, &dir.path().join("export")).unwrap();
    assert_eq!(exported, [dir.path().join("export").join("parabolica.db")]);
    assert_eq!(fs::read(&exported[0]).unwrap(), damaged);
    // Exporting again never overwrites the earlier copy.
    assert!(export_copy(&path, &dir.path().join("export")).is_err());

    let moved = move_aside(&path).unwrap();
    assert!(!path.exists());
    assert_eq!(fs::read(&moved).unwrap(), damaged);

    let fresh = Storage::open(&path).unwrap();
    assert_eq!(fresh.library_roots().unwrap(), Vec::new());
    assert_eq!(fresh.preference::<AppearanceMode>().unwrap(), None);
}

#[test]
fn a_foreign_sqlite_database_is_not_migrated() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("parabolica.db");
    let foreign = rusqlite::Connection::open(&path).unwrap();
    foreign
        .execute_batch("CREATE TABLE notes (body TEXT); INSERT INTO notes VALUES ('keep me');")
        .unwrap();
    drop(foreign);
    let before = fs::read(&path).unwrap();

    let error = Storage::open(&path).unwrap_err();
    assert!(
        matches!(
            &error,
            OpenError::Incompatible {
                reason: Incompatibility::NotParabolica,
                ..
            }
        ),
        "{error:?}"
    );
    assert_recoverable(&error);
    assert_eq!(fs::read(&path).unwrap(), before);
}

#[test]
fn a_database_from_a_newer_version_is_refused_without_changes() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("parabolica.db");
    populated(&path);
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute(
        "INSERT INTO schema_migrations (name, checksum, applied_at_ms) VALUES ('0099_01_future.sql', '0000000000000000', 0)",
        [],
    )
    .unwrap();
    drop(conn);
    let before = fs::read(&path).unwrap();

    let error = Storage::open(&path).unwrap_err();
    assert_eq!(
        error.to_string(),
        format!(
            "{} cannot be used by this version: it was created by a newer version (unknown migrations: 0099_01_future.sql)",
            path.display()
        )
    );
    assert_recoverable(&error);
    assert_eq!(fs::read(&path).unwrap(), before);
}

#[test]
fn a_second_instance_is_refused_until_the_first_closes() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("parabolica.db");
    let first = Storage::open(&path).unwrap();

    let error = Storage::open(&path).unwrap_err();
    assert!(matches!(error, OpenError::InUse { .. }), "{error:?}");
    assert_eq!(error.recovery_options(), [RecoveryOption::Retry]);
    assert!(matches!(move_aside(&path), Err(OpenError::InUse { .. })));

    drop(first);
    Storage::open(&path).unwrap();
}
