//! Store errors: every variant says what was attempted and with which values (constitution §3).

use std::fmt;
use std::path::{Path, PathBuf};

/// Anything the store can fail with.
#[derive(Debug)]
pub enum StoreError {
    /// SQLite failed while `op` ran against `path` (or a query named by `op`).
    Sqlite {
        /// What was being done.
        op: &'static str,
        /// The database file, when known.
        path: Option<PathBuf>,
        /// The underlying error.
        source: rusqlite::Error,
    },
    /// An op payload did not decode.
    Codec {
        /// The row's seq.
        seq: i64,
        /// The underlying error.
        source: postcard::Error,
    },
    /// The file was written by a newer txtodo.
    SchemaTooNew {
        /// Version in the file.
        found: i64,
        /// Version this build supports.
        supported: i64,
    },
    /// `append` was given no ops.
    EmptyBatch,
    /// `append` was given more than `MAX_APPEND_BATCH` ops.
    BatchTooLarge(usize),
    /// A stored path failed `FilePath::new` (the database was edited by hand).
    BadPath(String),
    /// `put_projection` was given more than `MAX_PROJECTION_BYTES`.
    ProjectionTooLarge(usize),
    /// A stored hash is not 32 bytes (the database was edited by hand); names the file.
    BadHash(String),
}

impl StoreError {
    pub(crate) fn sqlite(
        op: &'static str,
        path: &Path,
    ) -> impl FnOnce(rusqlite::Error) -> StoreError {
        let path = path.to_path_buf();
        move |source| StoreError::Sqlite {
            op,
            path: Some(path),
            source,
        }
    }
    pub(crate) fn query(op: &'static str) -> impl FnOnce(rusqlite::Error) -> StoreError {
        move |source| StoreError::Sqlite {
            op,
            path: None,
            source,
        }
    }
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StoreError::Sqlite {
                op,
                path: Some(p),
                source,
            } => write!(f, "{op} on {}: {source}", p.display()),
            StoreError::Sqlite {
                op,
                path: None,
                source,
            } => write!(f, "{op}: {source}"),
            StoreError::Codec { seq, source } => {
                write!(f, "decode op payload at seq {seq}: {source}")
            }
            StoreError::SchemaTooNew { found, supported } => {
                write!(
                    f,
                    "database schema version {found} is newer than supported {supported}"
                )
            }
            StoreError::EmptyBatch => write!(f, "append called with no ops"),
            StoreError::BatchTooLarge(n) => {
                write!(f, "append called with {n} ops, over the batch limit")
            }
            StoreError::BadPath(p) => write!(f, "stored file path {p:?} is invalid"),
            StoreError::ProjectionTooLarge(n) => {
                write!(f, "projection of {n} bytes is over the size limit")
            }
            StoreError::BadHash(file) => write!(f, "stored hash for {file} is not 32 bytes"),
        }
    }
}

impl std::error::Error for StoreError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            StoreError::Sqlite { source, .. } => Some(source),
            StoreError::Codec { source, .. } => Some(source),
            StoreError::SchemaTooNew { .. }
            | StoreError::EmptyBatch
            | StoreError::BatchTooLarge(_)
            | StoreError::BadPath(_)
            | StoreError::ProjectionTooLarge(_)
            | StoreError::BadHash(_) => None,
        }
    }
}
