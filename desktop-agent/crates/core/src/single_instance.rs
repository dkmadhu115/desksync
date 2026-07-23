//! Single-instance guard.
//!
//! Only one agent process may drive the machine's capture/input at a time; a
//! second instance would fight over the display and inject duplicate input.
//! We enforce this with an advisory exclusive lock on a lock file (via
//! `flock(2)` on Unix / `LockFile` on Windows). The lock is held for the
//! lifetime of the [`SingleInstance`] guard and released automatically on drop
//! (or process exit, which the OS handles even on a crash).

use crate::error::Result;
use fs4::fs_std::FileExt;
use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};

/// An acquired single-instance lock. Keep it alive for as long as the agent
/// should be considered "the" running instance; dropping it releases the lock.
#[derive(Debug)]
pub struct SingleInstance {
    file: File,
    path: PathBuf,
}

impl SingleInstance {
    /// Try to become the single running instance by locking `path`.
    ///
    /// Returns `Ok(Some(guard))` if this process acquired the lock, or
    /// `Ok(None)` if another instance already holds it.
    pub fn acquire(path: impl Into<PathBuf>) -> Result<Option<Self>> {
        let path = path.into();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)?;

        if file.try_lock_exclusive()? {
            Ok(Some(Self { file, path }))
        } else {
            Ok(None)
        }
    }

    /// The lock file path.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for SingleInstance {
    fn drop(&mut self) {
        // Best-effort explicit unlock; the OS also releases it on close/exit.
        let _ = FileExt::unlock(&self.file);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn second_acquisition_is_refused_then_released() {
        let dir = tempdir().unwrap();
        let lock = dir.path().join("agent.lock");

        let first = SingleInstance::acquire(&lock).unwrap();
        assert!(first.is_some(), "first instance should acquire the lock");

        let second = SingleInstance::acquire(&lock).unwrap();
        assert!(second.is_none(), "second instance must be refused");

        drop(first);
        let third = SingleInstance::acquire(&lock).unwrap();
        assert!(third.is_some(), "lock should be reacquirable after the holder drops");
    }
}
