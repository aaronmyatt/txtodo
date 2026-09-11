//! Single-instance lock: `<workspace>/.txtodo/txtodod.pid` holds our pid and an exclusive advisory
//! lock for the process lifetime. A second daemon on the same workspace exits with the running
//! pid in its message. https://doc.rust-lang.org/std/fs/struct.File.html#method.try_lock

use std::fmt;
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, Write};
use std::path::{Path, PathBuf};

/// The held lock; dropping it releases the lock and removes the file.
#[derive(Debug)]
pub struct PidFile {
    path: PathBuf,
    _file: File,
}

/// Why the lock was not taken.
#[derive(Debug)]
pub enum PidError {
    /// Another daemon holds the lock (its pid when readable).
    Running {
        /// The other process, if the file held a number.
        pid: Option<u32>,
        /// The pid file.
        path: PathBuf,
    },
    /// The file could not be opened, locked or written.
    Io {
        /// The pid file.
        path: PathBuf,
        /// The cause.
        source: std::io::Error,
    },
}

impl fmt::Display for PidError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PidError::Running { pid: Some(p), path } => write!(
                f,
                "txtodod already running (pid {p}), lock {}",
                path.display()
            ),
            PidError::Running { pid: None, path } => {
                write!(f, "txtodod already running, lock {}", path.display())
            }
            PidError::Io { path, source } => write!(f, "pid file {}: {source}", path.display()),
        }
    }
}

impl std::error::Error for PidError {}

impl PidFile {
    /// Takes the exclusive lock and writes our pid, or reports who holds it.
    pub fn acquire(path: &Path) -> Result<PidFile, PidError> {
        let io = |source| PidError::Io {
            path: path.to_path_buf(),
            source,
        };
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)
            .map_err(io)?;
        match file.try_lock() {
            Ok(()) => {}
            Err(std::fs::TryLockError::WouldBlock) => {
                let mut text = String::new();
                let _ = file.read_to_string(&mut text);
                return Err(PidError::Running {
                    pid: text.trim().parse().ok(),
                    path: path.to_path_buf(),
                });
            }
            Err(std::fs::TryLockError::Error(source)) => return Err(io(source)),
        }
        file.set_len(0).map_err(io)?;
        file.rewind().map_err(io)?;
        write!(file, "{}", std::process::id()).map_err(io)?;
        file.flush().map_err(io)?;
        debug_assert!(path.exists());
        Ok(PidFile {
            path: path.to_path_buf(),
            _file: file,
        })
    }

    /// The pid file path.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for PidFile {
    fn drop(&mut self) {
        // Best effort: the lock is released with the descriptor either way.
        let _removed = std::fs::remove_file(&self.path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn second_acquire_fails_with_the_running_pid_and_drop_releases() {
        let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let path = dir.path().join("txtodod.pid");
        let first = PidFile::acquire(&path).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(first.path(), path);
        match PidFile::acquire(&path) {
            Err(PidError::Running { pid, .. }) => assert_eq!(pid, Some(std::process::id())),
            other => panic!("expected Running, got {other:?}"),
        }
        drop(first);
        assert!(!path.exists(), "removed on drop");
        let again = PidFile::acquire(&path);
        assert!(again.is_ok());
    }
}
