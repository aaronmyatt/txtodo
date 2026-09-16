//! The only path that touches a document on disk: temp file beside the target, fsync, rename.
//! Readers see the old bytes or the new bytes, never a prefix (plan M3 crash safety). Same shape
//! as `txtodo-cli/src/store.rs`; copied, not shared — see ABSTRACTIONS.md.
//! https://doc.rust-lang.org/std/fs/fn.rename.html

use std::fmt;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

/// A filesystem step that failed, with what was attempted and on which path.
#[derive(Debug)]
pub struct WriteError {
    /// The step.
    pub op: &'static str,
    /// The path.
    pub path: PathBuf,
    /// The cause.
    pub source: io::Error,
}

impl fmt::Display for WriteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "cannot {} {}: {}",
            self.op,
            self.path.display(),
            self.source
        )
    }
}

impl std::error::Error for WriteError {}

fn err(op: &'static str, path: &Path) -> impl FnOnce(io::Error) -> WriteError {
    let path = path.to_path_buf();
    move |source| WriteError { op, path, source }
}

/// The temp file name used beside `path`; starts with `.txtodo-` so the watcher's ignore rule
/// (anything under or named `.txtodo…`) never routes it to an actor.
pub fn temp_path_for(path: &Path) -> PathBuf {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let tmp = path.with_file_name(format!(".txtodo-{name}.{}.tmp", std::process::id()));
    debug_assert_eq!(
        tmp.parent(),
        path.parent(),
        "temp file shares the directory"
    );
    debug_assert!(
        tmp.file_name()
            .is_some_and(|n| n.to_string_lossy().starts_with(".txtodo-"))
    );
    tmp
}

/// Writes `bytes` atomically to `path`. A thin span wrapper around `write_atomic_inner`
/// (`#[instrument]` on the real body overflows).
#[tracing::instrument(skip_all, fields(path = %path.display(), bytes = bytes.len()))]
pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), WriteError> {
    write_atomic_inner(path, bytes)
}

fn write_atomic_inner(path: &Path, bytes: &[u8]) -> Result<(), WriteError> {
    let tmp = temp_path_for(path);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(err("create directory", dir))?;
    }
    let mut file = std::fs::File::create(&tmp).map_err(err("create temp file", &tmp))?;
    file.write_all(bytes).map_err(err("write", &tmp))?;
    file.sync_all().map_err(err("fsync", &tmp))?;
    drop(file);
    std::fs::rename(&tmp, path).map_err(err("rename over", path))?;
    debug_assert!(!tmp.exists(), "rename consumed the temp file");
    debug_assert!(path.exists(), "target exists after rename");
    log_write_atomic_done();
    Ok(())
}

/// Split out so the event macro doesn't count against `write_atomic`'s own `#[instrument]` budget.
fn log_write_atomic_done() {
    tracing::debug!("write_atomic_done");
}

/// Reads a document; a missing file is empty bytes (the walker may register a path before the
/// first write, and todo.sh creates files on first use).
pub fn read_or_empty(path: &Path) -> Result<Vec<u8>, WriteError> {
    match std::fs::read(path) {
        Ok(bytes) => Ok(bytes),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(err("read", path)(e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_then_read_round_trips_and_leaves_no_temp_file() {
        let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
        let path = dir.path().join("nested").join("todo.txt");
        assert_eq!(read_or_empty(&path).unwrap_or_default(), b"");
        write_atomic(&path, b"(A) one\r\n").unwrap_or_else(|e| panic!("{e}"));
        write_atomic(&path, b"(A) one\r\nx two\r\n").unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(
            read_or_empty(&path).unwrap_or_default(),
            b"(A) one\r\nx two\r\n"
        );
        let leftovers: Vec<_> = std::fs::read_dir(path.parent().unwrap_or(dir.path()))
            .map(|rd| rd.flatten().map(|e| e.file_name()).collect())
            .unwrap_or_default();
        assert_eq!(leftovers.len(), 1, "{leftovers:?}");
    }
}
