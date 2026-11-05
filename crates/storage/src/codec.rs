//! Conversions between domain values and their column representation.

use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use parabolica_domain::files::{ContentDigest, FileSignature};
use parabolica_domain::ids::{MediaFileId, VirtualTrackId};
use parabolica_domain::provider::{ProviderId, ProviderItemId, ProviderItemRef};
use parabolica_domain::source::PlayableSource;
use rusqlite::Row;

use crate::error::StorageError;

pub(crate) fn now_ms() -> i64 {
    let elapsed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    i64::try_from(elapsed.as_millis()).unwrap_or(i64::MAX)
}

pub(crate) fn blob16(
    row: &Row<'_>,
    index: usize,
    column: &'static str,
) -> Result<[u8; 16], StorageError> {
    let bytes: Vec<u8> = row.get(index)?;
    bytes
        .try_into()
        .map_err(|_| StorageError::InvalidStoredValue { column })
}

pub(crate) fn optional_blob16(
    row: &Row<'_>,
    index: usize,
    column: &'static str,
) -> Result<Option<[u8; 16]>, StorageError> {
    let bytes: Option<Vec<u8>> = row.get(index)?;
    bytes
        .map(|bytes| bytes.try_into())
        .transpose()
        .map_err(|_| StorageError::InvalidStoredValue { column })
}

pub(crate) fn u64_to_sql(value: u64, what: &'static str) -> Result<i64, StorageError> {
    i64::try_from(value)
        .map_err(|_| StorageError::invalid(what, "exceeds the stored integer range"))
}

pub(crate) fn sql_to_u64(value: i64, column: &'static str) -> Result<u64, StorageError> {
    u64::try_from(value).map_err(|_| StorageError::InvalidStoredValue { column })
}

/// A path relative to a library root, stored with `/` separators so an exported database reads the
/// same on every platform.
pub(crate) fn relative_path_to_sql(path: &Path) -> Result<String, StorageError> {
    let mut parts = Vec::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => parts.push(part.to_str().ok_or(StorageError::invalid(
                "relative path",
                "is not valid Unicode",
            ))?),
            _ => {
                return Err(StorageError::invalid(
                    "relative path",
                    "must contain only normal components below its root",
                ));
            }
        }
    }
    if parts.is_empty() {
        return Err(StorageError::invalid("relative path", "is empty"));
    }
    Ok(parts.join("/"))
}

pub(crate) fn relative_path_from_sql(text: &str) -> PathBuf {
    text.split('/').collect()
}

pub(crate) fn path_to_sql(path: &Path) -> Result<&str, StorageError> {
    path.to_str()
        .ok_or(StorageError::invalid("path", "is not valid Unicode"))
}

pub(crate) struct SignatureColumns {
    pub size_bytes: i64,
    pub modified_unix_nanos: i64,
    pub digest: Option<[u8; 32]>,
}

pub(crate) fn signature_to_sql(
    signature: &FileSignature,
) -> Result<SignatureColumns, StorageError> {
    Ok(SignatureColumns {
        size_bytes: u64_to_sql(signature.size_bytes, "file size")?,
        modified_unix_nanos: i64::try_from(signature.modified_unix_nanos).map_err(|_| {
            StorageError::invalid("modification time", "is outside the years 1677 to 2262")
        })?,
        digest: signature.content_digest.map(|digest| digest.0),
    })
}

pub(crate) fn signature_from_sql(
    size_bytes: i64,
    modified_unix_nanos: i64,
    digest: Option<Vec<u8>>,
) -> Result<FileSignature, StorageError> {
    Ok(FileSignature {
        size_bytes: sql_to_u64(size_bytes, "size_bytes")?,
        modified_unix_nanos: i128::from(modified_unix_nanos),
        content_digest: digest
            .map(|bytes| bytes.try_into().map(ContentDigest))
            .transpose()
            .map_err(|_| StorageError::InvalidStoredValue {
                column: "content_digest",
            })?,
    })
}

/// A [`PlayableSource`] as `(kind, local id, provider, provider item)` columns.
pub(crate) struct SourceColumns<'a> {
    pub kind: &'static str,
    pub local_id: Option<[u8; 16]>,
    pub provider: Option<&'static str>,
    pub provider_item: Option<&'a str>,
}

pub(crate) fn source_to_sql(source: &PlayableSource) -> Result<SourceColumns<'_>, StorageError> {
    Ok(match source {
        PlayableSource::LocalFile(file) => SourceColumns {
            kind: "file",
            local_id: Some(file.to_bytes()),
            provider: None,
            provider_item: None,
        },
        PlayableSource::CueTrack(track) => SourceColumns {
            kind: "cue",
            local_id: Some(track.to_bytes()),
            provider: None,
            provider_item: None,
        },
        PlayableSource::Provider(item) => {
            if item.item.0.is_empty() {
                return Err(StorageError::invalid("provider item", "has an empty ID"));
            }
            SourceColumns {
                kind: "provider",
                local_id: None,
                provider: Some(item.provider.key()),
                provider_item: Some(&item.item.0),
            }
        }
    })
}

pub(crate) fn source_from_sql(
    kind: &str,
    local_id: Option<[u8; 16]>,
    provider: Option<String>,
    provider_item: Option<String>,
) -> Result<PlayableSource, StorageError> {
    let invalid = StorageError::InvalidStoredValue {
        column: "source_kind",
    };
    match (kind, local_id, provider, provider_item) {
        ("file", Some(id), None, None) => {
            Ok(PlayableSource::LocalFile(MediaFileId::from_bytes(id)))
        }
        ("cue", Some(id), None, None) => {
            Ok(PlayableSource::CueTrack(VirtualTrackId::from_bytes(id)))
        }
        ("provider", None, Some(provider), Some(item)) => {
            Ok(PlayableSource::Provider(ProviderItemRef {
                provider: ProviderId::from_key(&provider)
                    .ok_or(StorageError::InvalidStoredValue { column: "provider" })?,
                item: ProviderItemId(item),
            }))
        }
        _ => Err(invalid),
    }
}

/// Field and key names that indicate a credential. Credentials live in OS credential storage
/// (#14); the database, its exports and its error messages never hold them.
pub(crate) fn names_credential(name: &str) -> bool {
    const WORDS: [&str; 9] = [
        "token",
        "tokens",
        "secret",
        "password",
        "passwd",
        "cookie",
        "cookies",
        "credential",
        "credentials",
    ];
    let lower = name.to_ascii_lowercase();
    lower
        .split(|c: char| !c.is_ascii_alphanumeric())
        .any(|word| WORDS.contains(&word))
        || lower
            .replace(|c: char| !c.is_ascii_alphanumeric(), "")
            .contains("apikey")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_paths_round_trip_with_forward_slashes() {
        let path: PathBuf = ["Artist", "Album (Disc 1)", "01 – 曲.flac"]
            .iter()
            .collect();
        let stored = relative_path_to_sql(&path).unwrap();
        assert_eq!(stored, "Artist/Album (Disc 1)/01 – 曲.flac");
        assert_eq!(relative_path_from_sql(&stored), path);
    }

    #[test]
    fn relative_paths_must_stay_below_the_root() {
        for path in ["", "../escape.flac", "/absolute.flac", "a/../b.flac"] {
            assert!(
                relative_path_to_sql(Path::new(path)).is_err(),
                "{path} was accepted"
            );
        }
    }

    #[test]
    fn credential_names_are_recognised() {
        for name in [
            "youtube.oauth_token",
            "discogs.apiKey",
            "lastfm.api_key",
            "session-cookie",
            "Password",
        ] {
            assert!(names_credential(name), "{name}");
        }
        for name in [
            "appearance.mode",
            "replaygain.preamp_db",
            "tokenizer",
            "author",
        ] {
            assert!(!names_credential(name), "{name}");
        }
    }
}
