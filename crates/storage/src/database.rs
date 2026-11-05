//! The database owner: opening, the single writer thread and read-only connections.

use std::fs::{self, File, TryLockError};
use std::panic::{self, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use rusqlite::{Connection, OpenFlags, Transaction, TransactionBehavior};

use crate::codec::now_ms;
use crate::error::{OpenError, StorageError};
use crate::migrate::{self, Inspection, Migration};

/// Writes waiting for the writer thread before `write` blocks the submitter.
const WRITE_QUEUE_CAPACITY: usize = 64;
const IDLE_READERS: usize = 4;
const BUSY_TIMEOUT: Duration = Duration::from_secs(5);

type WriteJob = Box<dyn FnOnce(&mut Connection) + Send>;

/// Handle to the app database. Cloning shares the same writer and reader pool.
///
/// Every method blocks until the database answers. Call them from controllers and workers, never
/// from the UI thread. Each write method is one transaction on the single writer thread: it either
/// commits completely or leaves no trace, including when the closure panics.
#[derive(Clone)]
pub struct Storage {
    inner: Arc<Inner>,
}

struct Inner {
    path: PathBuf,
    writer: Option<SyncSender<WriteJob>>,
    writer_thread: Option<JoinHandle<()>>,
    readers: Mutex<Vec<Connection>>,
    /// Held for the life of the handle so a second app instance cannot open the same database.
    _lock: File,
}

impl std::fmt::Debug for Storage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Storage")
            .field("path", &self.inner.path)
            .finish_non_exhaustive()
    }
}

impl Storage {
    /// Open or create the database at `path` and apply pending migrations.
    ///
    /// An existing database that holds data is copied to a sibling
    /// `<name>.backup-<unix ms>-before-<migration>` file before migrating. A damaged, foreign or
    /// newer database is reported and left untouched.
    pub fn open(path: impl Into<PathBuf>) -> Result<Storage, OpenError> {
        Self::open_with(path.into(), migrate::MIGRATIONS)
    }

    pub(crate) fn open_with(path: PathBuf, migrations: &[Migration]) -> Result<Storage, OpenError> {
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            fs::create_dir_all(parent)?;
        }
        let lock = lock_database(&path)?;
        let existed = fs::metadata(&path).is_ok_and(|metadata| metadata.len() > 0);

        let open_error = |error: StorageError| match error {
            StorageError::Corrupt { detail } => OpenError::Corrupt {
                path: path.clone(),
                detail,
            },
            other => OpenError::Io(std::io::Error::other(other)),
        };

        let mut conn = Connection::open_with_flags(
            &path,
            OpenFlags::SQLITE_OPEN_READ_WRITE
                | OpenFlags::SQLITE_OPEN_CREATE
                | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .map_err(|error| open_error(error.into()))?;
        conn.busy_timeout(BUSY_TIMEOUT)
            .map_err(|error| open_error(error.into()))?;
        if existed {
            quick_check(&conn).map_err(open_error)?;
        }

        let pending = match migrate::inspect(&conn).map_err(open_error)? {
            Ok(Inspection::Fresh) => Ok(migrations.iter().collect()),
            Ok(Inspection::Existing { applied }) => migrate::pending(&applied, migrations),
            Err(reason) => Err(reason),
        }
        .map_err(|reason| OpenError::Incompatible {
            path: path.clone(),
            reason,
        })?;
        let has_data = existed && pending.len() < migrations.len();

        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = FULL;
             PRAGMA foreign_keys = ON;",
        )
        .map_err(|error| open_error(error.into()))?;

        if !pending.is_empty() {
            let backup = if has_data {
                Some(back_up(&conn, &path, pending[0].name).map_err(open_error)?)
            } else {
                None
            };
            migrate::apply(&mut conn, &pending).map_err(|(migration, source)| {
                OpenError::MigrationFailed {
                    migration,
                    backup,
                    source: Box::new(source),
                }
            })?;
        }

        let (writer, jobs) = mpsc::sync_channel(WRITE_QUEUE_CAPACITY);
        let writer_thread = thread::Builder::new()
            .name("storage-writer".into())
            .spawn(move || run_writer(conn, jobs))?;
        Ok(Storage {
            inner: Arc::new(Inner {
                path,
                writer: Some(writer),
                writer_thread: Some(writer_thread),
                readers: Mutex::new(Vec::new()),
                _lock: lock,
            }),
        })
    }

    pub fn path(&self) -> &Path {
        &self.inner.path
    }

    /// Run `apply` as one immediate transaction on the writer thread.
    pub(crate) fn write<T, F>(&self, apply: F) -> Result<T, StorageError>
    where
        T: Send + 'static,
        F: FnOnce(&Transaction<'_>) -> Result<T, StorageError> + Send + 'static,
    {
        let (reply, response) = mpsc::sync_channel(1);
        let job: WriteJob = Box::new(move |conn| {
            let outcome = panic::catch_unwind(AssertUnwindSafe(|| {
                let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
                let value = apply(&tx)?;
                tx.commit()?;
                Ok(value)
            }));
            // The submitter may have stopped waiting; the transaction outcome stands either way.
            let _ = reply.send(outcome.unwrap_or(Err(StorageError::WritePanicked)));
        });
        self.inner
            .writer
            .as_ref()
            .ok_or(StorageError::Closed)?
            .send(job)
            .map_err(|_| StorageError::Closed)?;
        response.recv().map_err(|_| StorageError::Closed)?
    }

    /// Run `query` in a read transaction on a read-only connection, so it sees one consistent
    /// snapshot and never waits for the writer.
    pub(crate) fn read<T>(
        &self,
        query: impl FnOnce(&Transaction<'_>) -> Result<T, StorageError>,
    ) -> Result<T, StorageError> {
        let pooled = self
            .inner
            .readers
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .pop();
        let mut conn = match pooled {
            Some(conn) => conn,
            None => open_reader(&self.inner.path)?,
        };
        let result = (|| {
            let tx = conn.transaction()?;
            let value = query(&tx)?;
            tx.finish()?;
            Ok(value)
        })();
        let mut readers = self
            .inner
            .readers
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if readers.len() < IDLE_READERS {
            readers.push(conn);
        }
        result
    }
}

impl Drop for Inner {
    fn drop(&mut self) {
        self.readers
            .get_mut()
            .unwrap_or_else(PoisonError::into_inner)
            .clear();
        drop(self.writer.take());
        if let Some(thread) = self.writer_thread.take() {
            // A panic outside a write job has nothing left to roll back.
            let _ = thread.join();
        }
    }
}

fn run_writer(mut conn: Connection, jobs: Receiver<WriteJob>) {
    for job in jobs {
        job(&mut conn);
    }
    // Clean shutdown: fold the WAL into the main file. Failure only leaves a larger WAL.
    let _ = conn.execute_batch("PRAGMA optimize; PRAGMA wal_checkpoint(TRUNCATE);");
}

fn open_reader(path: &Path) -> Result<Connection, StorageError> {
    let conn = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    conn.busy_timeout(BUSY_TIMEOUT)?;
    Ok(conn)
}

pub(crate) fn lock_database(path: &Path) -> Result<File, OpenError> {
    let mut lock_name = path.as_os_str().to_owned();
    lock_name.push(".lock");
    let lock = File::options()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(PathBuf::from(lock_name))?;
    match lock.try_lock() {
        Ok(()) => Ok(lock),
        Err(TryLockError::WouldBlock) => Err(OpenError::InUse {
            path: path.to_owned(),
        }),
        Err(TryLockError::Error(error)) => Err(OpenError::Io(error)),
    }
}

fn quick_check(conn: &Connection) -> Result<(), StorageError> {
    let result: String = conn.pragma_query_value(None, "quick_check", |row| row.get(0))?;
    if result == "ok" {
        Ok(())
    } else {
        Err(StorageError::Corrupt { detail: result })
    }
}

fn back_up(conn: &Connection, path: &Path, first_pending: &str) -> Result<PathBuf, StorageError> {
    let stem = first_pending.trim_end_matches(".sql");
    let mut name = path.file_name().unwrap_or_default().to_owned();
    name.push(format!(".backup-{}-before-{stem}", now_ms()));
    let backup = path.with_file_name(name);
    let target = backup
        .to_str()
        .ok_or(StorageError::invalid("backup path", "is not valid Unicode"))?;
    conn.execute("VACUUM INTO ?1", [target])?;
    Ok(backup)
}

#[cfg(test)]
mod tests {
    use parabolica_domain::queue::RepeatMode;

    use super::*;
    use crate::preferences::Preference;
    use crate::queue::QueueCheckpoint;

    struct Volume(u8);

    impl Preference for Volume {
        const KEY: &'static str = "test.volume";

        fn encode(&self) -> String {
            self.0.to_string()
        }

        fn decode(stored: &str) -> Option<Self> {
            stored.parse().ok().map(Volume)
        }
    }

    fn empty_queue() -> QueueCheckpoint {
        QueueCheckpoint {
            entries: Vec::new(),
            current: None,
            position: Duration::ZERO,
            repeat: RepeatMode::All,
            shuffle: None,
        }
    }

    fn table_exists(storage: &Storage, name: &'static str) -> bool {
        storage
            .read(move |tx| {
                Ok(tx.query_row(
                    "SELECT count(*) FROM sqlite_schema WHERE name = ?1",
                    [name],
                    |row| row.get::<_, i64>(0),
                )? == 1)
            })
            .unwrap()
    }

    #[test]
    fn a_failed_write_rolls_back_everything_it_did() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path().join("library.db")).unwrap();
        storage.save_preference(&Volume(40)).unwrap();

        let error = storage
            .write(|tx| {
                tx.execute(
                    "UPDATE preferences SET value = '99' WHERE key = 'test.volume'",
                    [],
                )?;
                tx.execute("DELETE FROM preferences", [])?;
                Err::<(), _>(StorageError::invalid("test", "injected failure"))
            })
            .unwrap_err();
        assert!(matches!(error, StorageError::Invalid { .. }));
        assert_eq!(storage.preference::<Volume>().unwrap().unwrap().0, 40);
    }

    #[test]
    fn a_panicking_write_rolls_back_and_the_writer_keeps_serving() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path().join("library.db")).unwrap();
        storage.save_preference(&Volume(40)).unwrap();

        let error = storage
            .write(|tx| -> Result<(), StorageError> {
                tx.execute("DELETE FROM preferences", [])?;
                panic!("injected panic inside a write");
            })
            .unwrap_err();
        assert!(matches!(error, StorageError::WritePanicked));
        assert_eq!(storage.preference::<Volume>().unwrap().unwrap().0, 40);

        storage.save_preference(&Volume(41)).unwrap();
        storage.save_queue(empty_queue()).unwrap();
        assert_eq!(storage.preference::<Volume>().unwrap().unwrap().0, 41);
    }

    #[test]
    fn a_failing_migration_leaves_the_database_and_a_backup_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("library.db");
        let storage = Storage::open(&path).unwrap();
        storage.save_preference(&Volume(40)).unwrap();
        drop(storage);

        let mut migrations = migrate::MIGRATIONS.to_vec();
        migrations.push(Migration {
            name: "9998_01_good.sql",
            sql: "CREATE TABLE extension_ok (id INTEGER PRIMARY KEY) STRICT;",
        });
        migrations.push(Migration {
            name: "9999_01_broken.sql",
            sql: "DELETE FROM preferences; CREATE TABLE extension_broken (id INTEGER PRIMARY KEY) STRICT; SELECT * FROM no_such_table;",
        });
        let error = Storage::open_with(path.clone(), &migrations).unwrap_err();
        let OpenError::MigrationFailed {
            migration, backup, ..
        } = error
        else {
            panic!("expected a migration failure, got {error:?}");
        };
        assert_eq!(migration, "9999_01_broken.sql");
        let backup = backup.expect("an existing database is backed up first");

        // Nothing from either pending migration was applied, including the one that succeeded.
        let storage = Storage::open(&path).unwrap();
        assert_eq!(storage.preference::<Volume>().unwrap().unwrap().0, 40);
        assert!(!table_exists(&storage, "extension_ok"));
        assert!(!table_exists(&storage, "extension_broken"));
        drop(storage);

        let backup = Storage::open(&backup).unwrap();
        assert_eq!(backup.preference::<Volume>().unwrap().unwrap().0, 40);
    }

    #[test]
    fn a_migration_that_breaks_a_foreign_key_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("library.db");
        drop(Storage::open(&path).unwrap());

        let mut migrations = migrate::MIGRATIONS.to_vec();
        migrations.push(Migration {
            name: "9999_01_orphan.sql",
            sql: "INSERT INTO track_sources (track_id, source_kind, provider, provider_item_id, link_state, linked_at_ms)
                  VALUES (zeroblob(16), 'provider', 'discogs', '1', 'confirmed', 0);",
        });
        let error = Storage::open_with(path.clone(), &migrations).unwrap_err();
        assert!(
            matches!(error, OpenError::MigrationFailed { .. }),
            "{error:?}"
        );

        let storage = Storage::open(&path).unwrap();
        let links: i64 = storage
            .read(
                |tx| Ok(tx.query_row("SELECT count(*) FROM track_sources", [], |row| row.get(0))?),
            )
            .unwrap();
        assert_eq!(links, 0);
    }

    #[test]
    fn later_migrations_apply_to_an_existing_database_and_keep_its_data() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("library.db");
        let storage = Storage::open(&path).unwrap();
        storage.save_preference(&Volume(40)).unwrap();
        drop(storage);

        let mut migrations = migrate::MIGRATIONS.to_vec();
        migrations.push(Migration {
            name: "9999_01_extension.sql",
            sql:
                "CREATE TABLE extension (key TEXT PRIMARY KEY REFERENCES preferences (key)) STRICT;
                  INSERT INTO extension SELECT key FROM preferences;",
        });
        let storage = Storage::open_with(path.clone(), &migrations).unwrap();
        assert_eq!(storage.preference::<Volume>().unwrap().unwrap().0, 40);
        let copied: i64 = storage
            .read(|tx| Ok(tx.query_row("SELECT count(*) FROM extension", [], |row| row.get(0))?))
            .unwrap();
        assert_eq!(copied, 1);
        drop(storage);

        // The current binary does not know 9999_01, so it refuses the newer database.
        let error = Storage::open(&path).unwrap_err();
        assert!(
            matches!(
                &error,
                OpenError::Incompatible {
                    reason: crate::Incompatibility::NewerSchema { .. },
                    ..
                }
            ),
            "{error:?}"
        );
        let backups = fs::read_dir(dir.path())
            .unwrap()
            .filter(|entry| {
                entry
                    .as_ref()
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .contains(".backup-")
            })
            .count();
        assert_eq!(backups, 1);
    }
}
