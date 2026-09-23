//! [`SpawnGuard`], split out of `spawn.rs`'s unix module for that file's line budget.

use crate::LaunchError;
use std::fs::{File, OpenOptions};
use std::io;
use std::path::Path;

/// Client-side no-double-spawn guard: an exclusive lock beside `sock`, held for the duration
/// of the absent-check-then-spawn so two `ensure_daemon` callers (in this process or another)
/// never both decide to spawn a daemon for the same socket.
/// Ref: <https://doc.rust-lang.org/std/fs/struct.File.html#method.lock>
pub(crate) struct SpawnGuard {
    _file: File,
}

impl SpawnGuard {
    pub(crate) async fn acquire(sock: &Path) -> Result<SpawnGuard, LaunchError> {
        let dir = sock
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(std::env::temp_dir);
        let path = dir.join("daemon-launch-spawn.lock");
        let file = tokio::task::spawn_blocking(move || -> io::Result<File> {
            std::fs::create_dir_all(&dir)?;
            let file = OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(false)
                .open(&path)?;
            file.lock()?;
            Ok(file)
        })
        .await
        .map_err(|_join_err| LaunchError::Lock(io::Error::other("spawn lock task panicked")))?
        .map_err(LaunchError::Lock)?;
        Ok(SpawnGuard { _file: file })
    }
}
