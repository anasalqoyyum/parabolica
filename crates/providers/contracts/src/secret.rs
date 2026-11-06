//! Authentication material and redaction.

use std::fmt;

use url::Url;
use zeroize::Zeroizing;

/// Authentication material: a token, cookie, session key or authorization code.
///
/// `Debug` prints `Secret([redacted])`, there is no `Display`, and the bytes are zeroed when the
/// last copy is dropped. Read the value with [`Secret::expose`] only where it is sent to the
/// service or the OS credential store.
#[derive(Clone)]
pub struct Secret(Zeroizing<Vec<u8>>);

impl Secret {
    pub fn new(bytes: impl Into<Vec<u8>>) -> Self {
        Self(Zeroizing::new(bytes.into()))
    }

    pub fn expose(&self) -> &[u8] {
        &self.0
    }

    /// The value as text, if it is UTF-8.
    pub fn expose_str(&self) -> Option<&str> {
        std::str::from_utf8(&self.0).ok()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl From<String> for Secret {
    fn from(value: String) -> Self {
        Self::new(value.into_bytes())
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret([redacted])")
    }
}

/// A URL reduced to its scheme and host, for errors and diagnostics.
///
/// Signed media URLs carry signatures and session parameters in the path as well as the query,
/// so neither is kept. User info and fragments are dropped too.
pub fn redact_url(url: &str) -> String {
    match Url::parse(url) {
        Ok(url) => match url.host_str() {
            Some(host) => format!("{}://{host}/[redacted]", url.scheme()),
            None => "[redacted URL]".to_owned(),
        },
        Err(_) => "[redacted URL]".to_owned(),
    }
}

/// Compares two byte strings in time that depends only on their lengths.
pub(crate) fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |diff, (x, y)| diff | (x ^ y)) == 0
}
