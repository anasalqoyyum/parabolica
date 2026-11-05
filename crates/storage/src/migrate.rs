//! Versioned schema migrations.
//!
//! Every `migrations/NNNN_MM_slug.sql` file is embedded at build time, sorted by name. The runner
//! records each applied migration by name with a checksum and applies all unapplied ones, in name
//! order, inside one transaction: a failure leaves the database exactly as it was.

use rusqlite::{Connection, OptionalExtension, params};

use crate::codec::now_ms;
use crate::error::{Incompatibility, StorageError};

/// `PBLA`, written to the SQLite header so a foreign database is never migrated.
pub(crate) const APPLICATION_ID: i32 = 0x5042_4c41;

#[derive(Debug, Clone, Copy)]
pub(crate) struct Migration {
    pub name: &'static str,
    pub sql: &'static str,
}

pub(crate) const MIGRATIONS: &[Migration] = include!(concat!(env!("OUT_DIR"), "/migrations.rs"));

impl Migration {
    /// FNV-1a over the SQL with carriage returns removed, so a Windows checkout with CRLF line
    /// endings matches a database created from an LF checkout.
    pub(crate) fn checksum(&self) -> String {
        let hash = self
            .sql
            .bytes()
            .filter(|byte| *byte != b'\r')
            .fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
                (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3)
            });
        format!("{hash:016x}")
    }
}

pub(crate) enum Inspection {
    Fresh,
    Existing { applied: Vec<(String, String)> },
}

/// Classify the database without modifying it.
pub(crate) fn inspect(
    conn: &Connection,
) -> Result<Result<Inspection, Incompatibility>, StorageError> {
    let application_id: i32 = conn.pragma_query_value(None, "application_id", |row| row.get(0))?;
    let object_count: i64 =
        conn.query_row("SELECT count(*) FROM sqlite_schema", [], |row| row.get(0))?;
    if application_id == 0 && object_count == 0 {
        return Ok(Ok(Inspection::Fresh));
    }
    if application_id != APPLICATION_ID {
        return Ok(Err(Incompatibility::NotParabolica));
    }
    let has_ledger: Option<i64> = conn
        .query_row(
            "SELECT 1 FROM sqlite_schema WHERE type = 'table' AND name = 'schema_migrations'",
            [],
            |row| row.get(0),
        )
        .optional()?;
    if has_ledger.is_none() {
        return Ok(Err(Incompatibility::NotParabolica));
    }
    let mut statement =
        conn.prepare("SELECT name, checksum FROM schema_migrations ORDER BY name")?;
    let applied = statement
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect::<Result<_, _>>()?;
    Ok(Ok(Inspection::Existing { applied }))
}

/// The known migrations not yet applied, or why this binary cannot use the database.
pub(crate) fn pending<'a>(
    applied: &[(String, String)],
    known: &'a [Migration],
) -> Result<Vec<&'a Migration>, Incompatibility> {
    let unknown: Vec<String> = applied
        .iter()
        .filter(|(name, _)| !known.iter().any(|migration| migration.name == name))
        .map(|(name, _)| name.clone())
        .collect();
    if !unknown.is_empty() {
        return Err(Incompatibility::NewerSchema { unknown });
    }
    if let Some(migration) = known.iter().find(|migration| {
        applied
            .iter()
            .any(|(name, checksum)| name == migration.name && *checksum != migration.checksum())
    }) {
        return Err(Incompatibility::ModifiedMigration {
            name: migration.name.to_owned(),
        });
    }
    Ok(known
        .iter()
        .filter(|migration| !applied.iter().any(|(name, _)| name == migration.name))
        .collect())
}

/// Apply `pending` in one transaction. On failure, returns the migration that failed.
///
/// Foreign keys are off while migrating so a migration can rebuild a table, and checked before
/// commit. Migration files must not contain their own transaction statements.
pub(crate) fn apply(
    conn: &mut Connection,
    pending: &[&Migration],
) -> Result<(), (String, StorageError)> {
    let Some(last) = pending.last() else {
        return Ok(());
    };
    let at = |name: &str| {
        let name = name.to_owned();
        move |error: rusqlite::Error| (name, StorageError::from(error))
    };
    conn.pragma_update(None, "foreign_keys", false)
        .map_err(at(last.name))?;
    let result = (|| {
        let tx = conn.transaction().map_err(at(last.name))?;
        tx.pragma_update(None, "application_id", APPLICATION_ID)
            .map_err(at(last.name))?;
        tx.execute_batch(
            "CREATE TABLE IF NOT EXISTS schema_migrations (
                name TEXT PRIMARY KEY,
                checksum TEXT NOT NULL,
                applied_at_ms INTEGER NOT NULL
            ) STRICT;",
        )
        .map_err(at(last.name))?;
        for migration in pending {
            tx.execute_batch(migration.sql)
                .map_err(at(migration.name))?;
            tx.execute(
                "INSERT INTO schema_migrations (name, checksum, applied_at_ms) VALUES (?1, ?2, ?3)",
                params![migration.name, migration.checksum(), now_ms()],
            )
            .map_err(at(migration.name))?;
        }
        let violation: Option<String> = tx
            .query_row(
                "SELECT \"table\" FROM pragma_foreign_key_check",
                [],
                |row| row.get(0),
            )
            .optional()
            .map_err(at(last.name))?;
        if violation.is_some() {
            return Err((
                last.name.to_owned(),
                StorageError::invalid("migration", "left rows that violate a foreign key"),
            ));
        }
        tx.commit().map_err(at(last.name))
    })();
    conn.pragma_update(None, "foreign_keys", true)
        .map_err(at(last.name))?;
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_migrations_are_sorted_and_start_with_the_core_schema() {
        assert_eq!(MIGRATIONS[0].name, "0004_01_core.sql");
        assert!(
            MIGRATIONS
                .windows(2)
                .all(|pair| pair[0].name < pair[1].name)
        );
    }

    #[test]
    fn checksum_ignores_carriage_returns() {
        let lf = Migration {
            name: "a",
            sql: "CREATE TABLE t (x);\nSELECT 1;\n",
        };
        let crlf = Migration {
            name: "a",
            sql: "CREATE TABLE t (x);\r\nSELECT 1;\r\n",
        };
        let edited = Migration {
            name: "a",
            sql: "CREATE TABLE t (y);\nSELECT 1;\n",
        };
        assert_eq!(lf.checksum(), crlf.checksum());
        assert_ne!(lf.checksum(), edited.checksum());
    }

    #[test]
    fn pending_applies_in_name_order_even_when_merged_out_of_order() {
        let known = [
            Migration {
                name: "0004_01_core.sql",
                sql: "",
            },
            Migration {
                name: "0006_01_library.sql",
                sql: "",
            },
            Migration {
                name: "0007_01_queue.sql",
                sql: "",
            },
        ];
        let applied = vec![
            ("0004_01_core.sql".to_owned(), known[0].checksum()),
            ("0007_01_queue.sql".to_owned(), known[2].checksum()),
        ];
        let names: Vec<_> = pending(&applied, &known)
            .unwrap()
            .iter()
            .map(|migration| migration.name)
            .collect();
        assert_eq!(names, ["0006_01_library.sql"]);
    }

    #[test]
    fn unknown_or_edited_applied_migrations_are_incompatible() {
        let known = [Migration {
            name: "0004_01_core.sql",
            sql: "x",
        }];
        let newer = vec![
            ("0004_01_core.sql".to_owned(), known[0].checksum()),
            ("0099_01_future.sql".to_owned(), "0".repeat(16)),
        ];
        assert_eq!(
            pending(&newer, &known).unwrap_err(),
            Incompatibility::NewerSchema {
                unknown: vec!["0099_01_future.sql".to_owned()]
            }
        );
        let edited = vec![("0004_01_core.sql".to_owned(), "0".repeat(16))];
        assert_eq!(
            pending(&edited, &known).unwrap_err(),
            Incompatibility::ModifiedMigration {
                name: "0004_01_core.sql".to_owned()
            }
        );
    }
}
