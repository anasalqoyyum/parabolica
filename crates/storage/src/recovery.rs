//! Recovery for a database that cannot be opened. Neither action modifies the original bytes.

use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};

use crate::codec::now_ms;
use crate::database::lock_database;
use crate::error::OpenError;

/// SQLite files that belong to a database: the main file, its write-ahead log, a hot rollback
/// journal and the WAL index.
const COMPANIONS: [&str; 4] = ["", "-wal", "-journal", "-shm"];

/// Copy the database and its log files into `destination_dir` without overwriting anything there.
/// Returns the copied paths. The shared-memory index is not copied; SQLite rebuilds it.
pub fn export_copy(database: &Path, destination_dir: &Path) -> Result<Vec<PathBuf>, OpenError> {
    let _lock = lock_database(database)?;
    let name = database.file_name().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "database path has no file name",
        )
    })?;
    fs::create_dir_all(destination_dir)?;
    let mut copied = Vec::new();
    for suffix in COMPANIONS.into_iter().filter(|suffix| *suffix != "-shm") {
        let source = with_suffix(database, suffix);
        if !source.exists() {
            continue;
        }
        let target = with_suffix(&destination_dir.join(name), suffix);
        let mut reader = File::open(&source)?;
        let mut writer = File::create_new(&target)?;
        io::copy(&mut reader, &mut writer)?;
        writer.sync_all()?;
        copied.push(target);
    }
    Ok(copied)
}

/// Rename the database and its companion files to `<name>.unusable-<unix ms>` so the next
/// [`crate::Storage::open`] creates an empty database. Returns the renamed database path.
pub fn move_aside(database: &Path) -> Result<PathBuf, OpenError> {
    let _lock = lock_database(database)?;
    let mut moved_name = database
        .file_name()
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "database path has no file name",
            )
        })?
        .to_owned();
    moved_name.push(format!(".unusable-{}", now_ms()));
    let moved = database.with_file_name(moved_name);

    // The main file moves last, so a fresh database never starts next to the old log files.
    let renames: Vec<(PathBuf, PathBuf)> = COMPANIONS
        .into_iter()
        .rev()
        .map(|suffix| (with_suffix(database, suffix), with_suffix(&moved, suffix)))
        .filter(|(source, _)| source.exists())
        .collect();
    if let Some((_, taken)) = renames.iter().find(|(_, target)| target.exists()) {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("{} already exists", taken.display()),
        )
        .into());
    }
    rename_all(&renames, |source, target| fs::rename(source, target))?;
    Ok(moved)
}

/// Apply `renames` in order. If one fails, undo the completed ones so the database stays with its
/// log files, and return the failure.
fn rename_all(
    renames: &[(PathBuf, PathBuf)],
    mut rename: impl FnMut(&Path, &Path) -> io::Result<()>,
) -> io::Result<()> {
    for (done, (source, target)) in renames.iter().enumerate() {
        if let Err(error) = rename(source, target) {
            for (restored_source, restored_target) in renames[..done].iter().rev() {
                rename(restored_target, restored_source).map_err(|restore_error| {
                    io::Error::other(format!(
                        "moving {} failed ({error}), and restoring {} from {} failed ({restore_error})",
                        source.display(),
                        restored_source.display(),
                        restored_target.display(),
                    ))
                })?;
            }
            return Err(error);
        }
    }
    Ok(())
}

fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_failed_rename_restores_the_files_already_moved() {
        let dir = tempfile::tempdir().unwrap();
        let database = dir.path().join("parabolica.db");
        let moved = dir.path().join("parabolica.db.unusable-1");
        let renames: Vec<_> = ["-wal", ""]
            .into_iter()
            .map(|suffix| (with_suffix(&database, suffix), with_suffix(&moved, suffix)))
            .collect();
        for (source, _) in &renames {
            fs::write(source, source.display().to_string()).unwrap();
        }

        let error = rename_all(&renames, |source, target| {
            if source == database {
                Err(io::Error::new(io::ErrorKind::PermissionDenied, "injected"))
            } else {
                fs::rename(source, target)
            }
        })
        .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
        for (source, target) in &renames {
            assert_eq!(
                fs::read_to_string(source).unwrap(),
                source.display().to_string()
            );
            assert!(!target.exists());
        }
    }
}
