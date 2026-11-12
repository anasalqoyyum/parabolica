//! Files named on the command line, as Explorer's Open with passes them, and the bytes a later
//! launch sends to the running instance.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

const MAGIC: &[u8; 8] = b"PBLOPEN1";
/// Explorer passes at most a few hundred files; a larger request is not from it.
pub const MAX_PATHS: usize = 4096;
/// The longest path storage accepts.
const MAX_PATH_BYTES: usize = 32_768;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OpenRequest {
    /// Absolute, Unicode paths in the order given. Empty asks only to show the running window.
    pub paths: Vec<PathBuf>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestError {
    Malformed,
    TooManyPaths,
    /// Not absolute, not Unicode, too long, or containing a line break or NUL.
    InvalidPath,
}

impl OpenRequest {
    /// Resolves relative arguments against `working_dir`. Arguments that storage cannot record
    /// (non-Unicode, too long, control characters) are dropped.
    pub fn from_args(args: impl IntoIterator<Item = OsString>, working_dir: Option<&Path>) -> Self {
        let paths = args
            .into_iter()
            .map(PathBuf::from)
            .filter_map(|path| {
                if path.is_absolute() {
                    Some(path)
                } else {
                    working_dir.map(|dir| dir.join(path))
                }
            })
            .filter(|path| valid_path(path))
            .take(MAX_PATHS)
            .collect();
        Self { paths }
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut bytes = MAGIC.to_vec();
        bytes.extend((self.paths.len() as u32).to_le_bytes());
        for path in &self.paths {
            let text = path.to_str().expect("open requests hold Unicode paths");
            bytes.extend((text.len() as u32).to_le_bytes());
            bytes.extend(text.as_bytes());
        }
        bytes
    }

    /// Rejects the whole request if any part is invalid, so a damaged request opens nothing.
    pub fn decode(bytes: &[u8]) -> Result<Self, RequestError> {
        let mut rest = bytes
            .strip_prefix(MAGIC.as_slice())
            .ok_or(RequestError::Malformed)?;
        let count = take_u32(&mut rest)? as usize;
        if count > MAX_PATHS {
            return Err(RequestError::TooManyPaths);
        }
        let mut paths = Vec::with_capacity(count);
        for _ in 0..count {
            let length = take_u32(&mut rest)? as usize;
            if length > MAX_PATH_BYTES || length > rest.len() {
                return Err(RequestError::InvalidPath);
            }
            let (text, remaining) = rest.split_at(length);
            rest = remaining;
            let path =
                PathBuf::from(std::str::from_utf8(text).map_err(|_| RequestError::InvalidPath)?);
            if !valid_path(&path) {
                return Err(RequestError::InvalidPath);
            }
            paths.push(path);
        }
        if rest.is_empty() {
            Ok(Self { paths })
        } else {
            Err(RequestError::Malformed)
        }
    }
}

fn take_u32(rest: &mut &[u8]) -> Result<u32, RequestError> {
    let (value, remaining) = rest.split_first_chunk().ok_or(RequestError::Malformed)?;
    *rest = remaining;
    Ok(u32::from_le_bytes(*value))
}

/// The paths storage can register: absolute, Unicode, at most 32 KiB, no line breaks or NUL, and
/// not a Windows device namespace path such as `\\.\pipe\...`.
fn valid_path(path: &Path) -> bool {
    path.is_absolute()
        && path.to_str().is_some_and(|text| {
            !text.is_empty()
                && text.len() <= MAX_PATH_BYTES
                && !text.contains(['\n', '\r', '\0'])
                && !text.starts_with(r"\\.\")
        })
}
