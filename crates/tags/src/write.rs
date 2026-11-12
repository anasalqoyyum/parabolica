//! Safe single-file writes: temporary sibling, verification, backup, then one rename.
//!
//! The original stays untouched until the final rename. Every failure before it leaves the
//! original as it was and removes the temporary file. A write is refused when the file no longer
//! matches the full-content signature recorded at review.

use std::fs::{self, File, OpenOptions};
use std::io::{self, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use parabolica_domain::files::{ContentDigest, FileSignature};
use parabolica_domain::tags::{FieldChange, PictureType};
use sha2::{Digest, Sha256};

use crate::artwork::{StagedImage, existing_sidecars};
use crate::fields;
use crate::flac::{self, Block, PICTURE, STREAMINFO, VORBIS_COMMENT, VorbisComments};
use crate::{FileTags, TagError};

/// Points around the replacement where tests inject failures or concurrent edits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// The temporary file is written and synced but not yet verified.
    TempWritten,
    /// The backup copy of the original exists.
    BackupWritten,
    /// Immediately before the rename that replaces the original.
    BeforeReplace,
}

pub trait Hooks {
    fn at(&self, step: Step, target: &Path) -> io::Result<()>;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct NoHooks;

impl Hooks for NoHooks {
    fn at(&self, _: Step, _: &Path) -> io::Result<()> {
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum WriteError {
    /// The file differs from the reviewed (or last written) content. Nothing was written.
    #[error("Changed since review")]
    Changed,
    /// Another program holds the file open in a way that prevents replacement.
    #[error("{0}")]
    InUse(String),
    #[error("{0}")]
    Failed(String),
}

/// What one file should receive. Pictures replace every existing picture of the same type.
#[derive(Debug, Clone, Copy)]
pub struct FileEdit<'a> {
    pub fields: &'a [FieldChange],
    pub pictures: &'a [(PictureType, &'a StagedImage)],
}

/// Size, modification time and the SHA-256 of the whole file.
pub fn fingerprint(path: &Path) -> io::Result<FileSignature> {
    let file = File::open(path)?;
    let metadata = file.metadata()?;
    let mut hasher = Sha256::new();
    io::copy(&mut BufReader::new(file), &mut HashWriter(&mut hasher))?;
    Ok(FileSignature {
        size_bytes: metadata.len(),
        modified_unix_nanos: modified_nanos(&metadata),
        content_digest: Some(ContentDigest(hasher.finalize().into())),
    })
}

/// Read comments and picture summaries without decoding audio.
pub fn read_tags(path: &Path) -> Result<FileTags, TagError> {
    let mut reader = BufReader::new(File::open(path)?);
    FileTags::from_metadata(&flac::read_metadata(&mut reader)?)
}

/// A verified temporary file and backup, waiting for [`Prepared::commit`].
#[derive(Debug)]
pub struct Prepared {
    target: PathBuf,
    temp: PathBuf,
    backup: PathBuf,
    reviewed: FileSignature,
    digest: ContentDigest,
}

impl Prepared {
    pub fn backup(&self) -> &Path {
        &self.backup
    }

    /// The content the target will have after the rename. Size and digest are final; the
    /// modification time is that of the temporary file.
    pub fn expected(&self) -> io::Result<FileSignature> {
        let metadata = fs::metadata(&self.temp)?;
        Ok(FileSignature {
            size_bytes: metadata.len(),
            modified_unix_nanos: modified_nanos(&metadata),
            content_digest: Some(self.digest),
        })
    }

    /// Replace the target. Fails with [`WriteError::Changed`] if its size or modification time
    /// moved after preparation. On failure the backup is removed, since nothing was written.
    pub fn commit(self, hooks: &dyn Hooks) -> Result<FileSignature, WriteError> {
        let result = (|| {
            hooks
                .at(Step::BeforeReplace, &self.target)
                .map_err(|error| failure(error, "Write failed"))?;
            let current = fs::metadata(&self.target).map_err(|e| failure(e, "File unavailable"))?;
            if current.len() != self.reviewed.size_bytes
                || modified_nanos(&current) != self.reviewed.modified_unix_nanos
            {
                return Err(WriteError::Changed);
            }
            fs::rename(&self.temp, &self.target).map_err(|error| {
                // Windows refuses to replace a file another process has open without
                // delete sharing. The temporary file in the same folder was writable, so this
                // is not a permissions problem.
                if cfg!(windows) && error.kind() == io::ErrorKind::PermissionDenied {
                    WriteError::InUse("File is open in another program".into())
                } else {
                    failure(error, "Replace failed")
                }
            })?;
            sync_parent(&self.target);
            let written = fs::metadata(&self.target).map_err(|e| failure(e, "File unavailable"))?;
            Ok(FileSignature {
                size_bytes: written.len(),
                modified_unix_nanos: modified_nanos(&written),
                content_digest: Some(self.digest),
            })
        })();
        if result.is_err() {
            let _ = fs::remove_file(&self.temp);
            let _ = fs::remove_file(&self.backup);
        }
        result
    }

    /// Remove the temporary file and backup without touching the target.
    pub fn abandon(self) {
        let _ = fs::remove_file(&self.temp);
        let _ = fs::remove_file(&self.backup);
    }
}

#[derive(Debug)]
pub enum Preparation {
    Ready(Prepared),
    /// The edit leaves the file's bytes as they are; nothing needs writing.
    Unchanged,
}

/// Build and verify the edited file beside the target, and back up the original.
///
/// `reviewed` must carry a full-content digest. The original is hashed while it is copied, so a
/// concurrent edit that kept size and modification time is still detected.
pub fn prepare(
    target: &Path,
    reviewed: &FileSignature,
    edit: FileEdit<'_>,
    temp: &Path,
    backup: &Path,
    hooks: &dyn Hooks,
) -> Result<Preparation, WriteError> {
    let Some(reviewed_digest) = reviewed.content_digest else {
        return Err(WriteError::Failed(
            "The reviewed signature has no content digest".into(),
        ));
    };
    let metadata = fs::metadata(target).map_err(|e| failure(e, "File unavailable"))?;
    if metadata.permissions().readonly() {
        return Err(WriteError::Failed("File is read-only".into()));
    }
    if metadata.len() != reviewed.size_bytes
        || modified_nanos(&metadata) != reviewed.modified_unix_nanos
    {
        return Err(WriteError::Changed);
    }
    let result = build_temp(target, reviewed_digest, edit, temp, hooks).and_then(|built| {
        let Some(digest) = built else {
            return Ok(Preparation::Unchanged);
        };
        copy_synced(target, backup).map_err(|e| failure(e, "Backup failed"))?;
        hooks
            .at(Step::BackupWritten, target)
            .map_err(|e| failure(e, "Backup failed"))?;
        if fs::metadata(backup)
            .map_err(|e| failure(e, "Backup failed"))?
            .len()
            != metadata.len()
        {
            return Err(WriteError::Failed("Backup is incomplete".into()));
        }
        Ok(Preparation::Ready(Prepared {
            target: target.to_owned(),
            temp: temp.to_owned(),
            backup: backup.to_owned(),
            reviewed: reviewed.clone(),
            digest,
        }))
    });
    if !matches!(result, Ok(Preparation::Ready(_))) {
        let _ = fs::remove_file(temp);
        let _ = fs::remove_file(backup);
    }
    result
}

/// Writes the temporary file and returns its digest, or `None` when the edit changes nothing.
fn build_temp(
    target: &Path,
    reviewed_digest: ContentDigest,
    edit: FileEdit<'_>,
    temp: &Path,
    hooks: &dyn Hooks,
) -> Result<Option<ContentDigest>, WriteError> {
    let mut source = HashReader {
        inner: BufReader::new(File::open(target).map_err(|e| failure(e, "Open failed"))?),
        hasher: Sha256::new(),
    };
    let metadata = flac::read_metadata(&mut source).map_err(tag_failure)?;
    let blocks = edited_blocks(&metadata.blocks, edit).map_err(tag_failure)?;
    let header = flac::encode_metadata(&blocks).map_err(tag_failure)?;
    let mut audio = Sha256::new();
    if blocks == metadata.blocks {
        io::copy(&mut source, &mut io::sink()).map_err(|e| failure(e, "Read failed"))?;
        return if ContentDigest(source.hasher.finalize().into()) == reviewed_digest {
            Ok(None)
        } else {
            Err(WriteError::Changed)
        };
    }
    let mut out = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(temp)
        .map_err(|e| failure(e, "Write failed"))?;
    let mut written = Sha256::new();
    let write = (|| {
        out.write_all(&header)?;
        written.update(&header);
        let mut buffer = vec![0; 256 * 1024];
        loop {
            let n = source.read(&mut buffer)?;
            if n == 0 {
                break;
            }
            out.write_all(&buffer[..n])?;
            audio.update(&buffer[..n]);
            written.update(&buffer[..n]);
        }
        out.sync_all()
    })();
    drop(out);
    write.map_err(|e| failure(e, "Write failed"))?;
    if ContentDigest(source.hasher.finalize().into()) != reviewed_digest {
        return Err(WriteError::Changed);
    }
    hooks
        .at(Step::TempWritten, target)
        .map_err(|e| failure(e, "Write failed"))?;
    verify_temp(temp, &blocks, &audio.finalize())?;
    Ok(Some(ContentDigest(written.finalize().into())))
}

/// Re-read the temporary file: its blocks must equal the intended blocks and its audio frames the
/// original frames, byte for byte.
fn verify_temp(temp: &Path, blocks: &[Block], audio_digest: &[u8]) -> Result<(), WriteError> {
    let mut reader = BufReader::new(File::open(temp).map_err(|e| failure(e, "Verify failed"))?);
    let written = flac::read_metadata(&mut reader).map_err(tag_failure)?;
    if written.blocks != blocks {
        return Err(WriteError::Failed(
            "Verification failed: metadata differs".into(),
        ));
    }
    let mut audio = Sha256::new();
    io::copy(&mut reader, &mut HashWriter(&mut audio)).map_err(|e| failure(e, "Verify failed"))?;
    if audio.finalize().as_slice() != audio_digest {
        return Err(WriteError::Failed(
            "Verification failed: audio differs".into(),
        ));
    }
    Ok(())
}

pub(crate) fn edited_blocks(blocks: &[Block], edit: FileEdit<'_>) -> Result<Vec<Block>, TagError> {
    let mut blocks = blocks.to_vec();
    if !edit.fields.is_empty() {
        let existing = blocks.iter().position(|b| b.kind == VORBIS_COMMENT);
        let index = match existing {
            Some(index) => index,
            None => {
                let at = blocks
                    .iter()
                    .position(|b| b.kind != STREAMINFO)
                    .unwrap_or(blocks.len());
                let empty = VorbisComments {
                    vendor: b"Parabolica".to_vec(),
                    entries: Vec::new(),
                };
                blocks.insert(
                    at,
                    Block {
                        kind: VORBIS_COMMENT,
                        data: empty.encode(),
                    },
                );
                at
            }
        };
        let mut comments = VorbisComments::decode(&blocks[index].data)?;
        let before = comments.clone();
        fields::apply(&mut comments, edit.fields)?;
        if comments != before {
            blocks[index].data = comments.encode();
        } else if existing.is_none() {
            blocks.remove(index);
        }
    }
    let mut kinds: Vec<PictureType> = Vec::new();
    for (kind, _) in edit.pictures {
        if !kinds.contains(kind) {
            kinds.push(*kind);
        }
    }
    for kind in kinds {
        let code = flac::picture_code(kind);
        let new: Vec<Block> = edit
            .pictures
            .iter()
            .filter(|(k, _)| *k == kind)
            .map(|(_, image)| Block {
                kind: PICTURE,
                data: image.picture(kind).encode(),
            })
            .collect();
        let existing: Vec<usize> = blocks
            .iter()
            .enumerate()
            .filter(|(_, b)| {
                b.kind == PICTURE && flac::Picture::decode(&b.data).is_ok_and(|p| p.code == code)
            })
            .map(|(i, _)| i)
            .collect();
        if existing.len() == new.len()
            && existing
                .iter()
                .zip(&new)
                .all(|(i, block)| blocks[*i] == *block)
        {
            continue;
        }
        let at = existing.first().copied().unwrap_or_else(|| {
            // After the last picture, or before trailing padding.
            blocks
                .iter()
                .rposition(|b| b.kind == PICTURE)
                .map(|i| i + 1)
                .or_else(|| blocks.iter().rposition(|b| b.kind != 1).map(|i| i + 1))
                .unwrap_or(blocks.len())
        });
        let removed_before = existing.iter().filter(|i| **i < at).count();
        let mut index = 0;
        blocks.retain(|_| {
            let keep = !existing.contains(&index);
            index += 1;
            keep
        });
        let at = at - removed_before;
        blocks.splice(at..at, new);
    }
    Ok(blocks)
}

/// Copy `backup` over `target` when the target still has the content written by Parabolica.
pub fn restore(
    target: &Path,
    backup: &Path,
    written: &FileSignature,
    temp: &Path,
) -> Result<FileSignature, WriteError> {
    let current = fingerprint(target).map_err(|e| failure(e, "File unavailable"))?;
    if current.size_bytes != written.size_bytes || current.content_digest != written.content_digest
    {
        return Err(WriteError::Changed);
    }
    let result = (|| {
        copy_synced(backup, temp).map_err(|e| failure(e, "Restore failed"))?;
        fs::rename(temp, target).map_err(|e| failure(e, "Restore failed"))?;
        sync_parent(target);
        fingerprint(target).map_err(|e| failure(e, "File unavailable"))
    })();
    if result.is_err() {
        let _ = fs::remove_file(temp);
    }
    result
}

/// Write `image` as `dir/name`. `replace` lists the existing sidecars reviewed for replacement;
/// they are backed up into `backup_dir` and removed. A sidecar that appeared after review fails
/// the write with [`WriteError::Changed`].
pub fn write_sidecar(
    dir: &Path,
    name: &str,
    kind: PictureType,
    image: &StagedImage,
    replace: &[String],
    backup_dir: &Path,
    temp_suffix: &str,
) -> Result<(), WriteError> {
    let existing = existing_sidecars(dir, kind).map_err(|e| failure(e, "Folder unavailable"))?;
    if existing.iter().any(|name| !replace.contains(name)) {
        return Err(WriteError::Changed);
    }
    let target = dir.join(name);
    // On a case-insensitive folder an existing `Cover.jpg` is the target `cover.jpg`.
    if !existing.iter().any(|e| e.eq_ignore_ascii_case(name)) && target.exists() {
        return Err(WriteError::Changed);
    }
    for existing in &existing {
        copy_synced(&dir.join(existing), &backup_dir.join(existing))
            .map_err(|e| failure(e, "Backup failed"))?;
    }
    let temp = dir.join(format!(".{name}.{temp_suffix}.parabolica-tmp"));
    let written = (|| {
        let mut file = File::create(&temp)?;
        file.write_all(image.bytes())?;
        file.sync_all()
    })();
    if let Err(error) = written {
        let _ = fs::remove_file(&temp);
        return Err(failure(error, "Write failed"));
    }
    // Remove the reviewed files first, so the new name is the only one left on case-sensitive
    // and case-insensitive folders alike. Their backups are put back if the rename fails.
    let mut removed = Vec::new();
    let mut result = Ok(());
    for existing in &existing {
        match fs::remove_file(dir.join(existing)) {
            Ok(()) => removed.push(existing),
            Err(error) => {
                result = Err(error);
                break;
            }
        }
    }
    let result = result.and_then(|()| fs::rename(&temp, &target));
    if let Err(error) = result {
        let _ = fs::remove_file(&temp);
        for existing in removed {
            let _ = copy_synced(&backup_dir.join(existing), &dir.join(existing));
        }
        return Err(failure(error, "Write failed"));
    }
    sync_parent(&target);
    Ok(())
}

pub(crate) fn modified_nanos(metadata: &fs::Metadata) -> i128 {
    match metadata.modified() {
        Ok(time) => match time.duration_since(UNIX_EPOCH) {
            Ok(after) => after.as_nanos() as i128,
            Err(before) => -(before.duration().as_nanos() as i128),
        },
        Err(_) => 0,
    }
}

fn copy_synced(from: &Path, to: &Path) -> io::Result<()> {
    if let Some(parent) = to.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut input = File::open(from)?;
    let mut output = File::create(to)?;
    io::copy(&mut input, &mut output)?;
    output.sync_all()
}

fn sync_parent(path: &Path) {
    // Directory handles cannot be opened this way on Windows, where the rename is already durable
    // once it returns.
    #[cfg(unix)]
    if let Some(parent) = path.parent()
        && let Ok(dir) = File::open(parent)
    {
        let _ = dir.sync_all();
    }
    #[cfg(not(unix))]
    let _ = path;
}

fn tag_failure(error: TagError) -> WriteError {
    match error {
        TagError::Io(error) => failure(error, "Read failed"),
        other => WriteError::Failed(other.to_string()),
    }
}

/// Map an I/O error to an outcome. Sharing and lock violations mean another program has the file
/// open, so the write can be retried later.
pub(crate) fn failure(error: io::Error, action: &str) -> WriteError {
    #[cfg(windows)]
    const IN_USE: [i32; 2] = [32, 33];
    #[cfg(not(windows))]
    const IN_USE: [i32; 0] = [];
    if error
        .raw_os_error()
        .is_some_and(|code| IN_USE.contains(&code))
    {
        return WriteError::InUse("File is open in another program".into());
    }
    let message = match error.kind() {
        io::ErrorKind::StorageFull => "Disk is full".to_owned(),
        io::ErrorKind::QuotaExceeded => "Disk quota exceeded".to_owned(),
        io::ErrorKind::ReadOnlyFilesystem => "Drive is read-only".to_owned(),
        io::ErrorKind::PermissionDenied => "Permission denied".to_owned(),
        io::ErrorKind::NotFound => "File not found".to_owned(),
        _ => format!("{action}: {error}"),
    };
    WriteError::Failed(message)
}

struct HashWriter<'a>(&'a mut Sha256);

impl Write for HashWriter<'_> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.update(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

struct HashReader<R> {
    inner: R,
    hasher: Sha256,
}

impl<R: Read> Read for HashReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = self.inner.read(buf)?;
        self.hasher.update(&buf[..n]);
        Ok(n)
    }
}
