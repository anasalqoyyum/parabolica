use std::fs::File;
use std::io::{self, Read};
use std::path::Path;
use std::time::UNIX_EPOCH;

use parabolica_domain::files::{ContentDigest, FileSignature};
use sha2::{Digest, Sha256};

/// Full-file SHA-256 with cancellation/playback scheduling between 64 KiB blocks.
pub fn fingerprint(
    path: &Path,
    mut checkpoint: impl FnMut() -> io::Result<()>,
) -> io::Result<FileSignature> {
    let mut file = File::open(path)?;
    let before = file.metadata()?;
    let mut hash = Sha256::new();
    let mut buffer = [0; 65536];
    loop {
        checkpoint()?;
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
    }
    let after = file.metadata()?;
    let modified = |m: &std::fs::Metadata| {
        m.modified()?
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos() as i128)
            .map_err(io::Error::other)
    };
    if before.len() != after.len() || modified(&before)? != modified(&after)? {
        return Err(io::Error::other("File changed while reading; rescan it"));
    }
    Ok(FileSignature {
        size_bytes: after.len(),
        modified_unix_nanos: modified(&after)?,
        content_digest: Some(ContentDigest(hash.finalize().into())),
    })
}
