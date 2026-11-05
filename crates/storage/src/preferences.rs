//! Typed preferences saved atomically.
//!
//! The owning feature defines each preference as a type implementing [`Preference`]. Keys are
//! lower-case dotted names (`appearance.mode`). Credentials are never preferences: keys that name
//! one are rejected, and secrets belong in OS credential storage.

use std::collections::BTreeMap;

use rusqlite::{OptionalExtension, params};

use crate::codec::{names_credential, now_ms};
use crate::{Storage, StorageError};

pub trait Preference: Sized {
    const KEY: &'static str;

    fn encode(&self) -> String;

    /// `None` when the stored text is not a valid value, for example after a downgrade.
    fn decode(stored: &str) -> Option<Self>;
}

/// Preferences saved together in one transaction.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct PreferenceBatch {
    values: Vec<(&'static str, String)>,
}

impl PreferenceBatch {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set<P: Preference>(mut self, value: &P) -> Self {
        self.values.retain(|(key, _)| *key != P::KEY);
        self.values.push((P::KEY, value.encode()));
        self
    }
}

impl Storage {
    /// The saved value of `P`, `None` when unset, or [`StorageError::InvalidStoredValue`] when the
    /// stored text does not decode.
    pub fn preference<P: Preference>(&self) -> Result<Option<P>, StorageError> {
        validate_key(P::KEY)?;
        let stored: Option<String> = self.read(|tx| {
            Ok(tx
                .query_row(
                    "SELECT value FROM preferences WHERE key = ?1",
                    [P::KEY],
                    |row| row.get(0),
                )
                .optional()?)
        })?;
        stored
            .map(|text| {
                P::decode(&text).ok_or(StorageError::InvalidStoredValue {
                    column: "preferences.value",
                })
            })
            .transpose()
    }

    pub fn save_preference<P: Preference>(&self, value: &P) -> Result<(), StorageError> {
        self.save_preferences(PreferenceBatch::new().set(value))
    }

    /// Save every preference in `batch`, or none of them.
    pub fn save_preferences(&self, batch: PreferenceBatch) -> Result<(), StorageError> {
        for (key, _) in &batch.values {
            validate_key(key)?;
        }
        self.write(move |tx| {
            let mut upsert = tx.prepare(
                "INSERT INTO preferences (key, value, updated_at_ms) VALUES (?1, ?2, ?3)
                 ON CONFLICT (key) DO UPDATE SET value = excluded.value, updated_at_ms = excluded.updated_at_ms",
            )?;
            let saved_at = now_ms();
            for (key, value) in &batch.values {
                upsert.execute(params![key, value, saved_at])?;
            }
            Ok(())
        })
    }

    /// Every saved preference as stored text, for export.
    pub fn all_preferences(&self) -> Result<BTreeMap<String, String>, StorageError> {
        self.read(|tx| {
            let mut statement = tx.prepare("SELECT key, value FROM preferences")?;
            let values = statement
                .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
                .collect::<Result<_, _>>()?;
            Ok(values)
        })
    }
}

fn validate_key(key: &str) -> Result<(), StorageError> {
    let well_formed = !key.is_empty()
        && key.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"._-".contains(&byte)
        });
    if !well_formed {
        return Err(StorageError::invalid(
            "preference key",
            "must be lower-case letters, digits, '.', '_' or '-'",
        ));
    }
    if names_credential(key) {
        return Err(StorageError::invalid(
            "preference key",
            "names a credential; use OS credential storage",
        ));
    }
    Ok(())
}
