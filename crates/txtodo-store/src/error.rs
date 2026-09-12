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
    /// A stored device id is not 16 bytes (the database was edited by hand); the length found.
    BadDevice(usize),
    /// `ops_for` was given an empty, inverted or over-wide run (1-based, ≤ `MAX_OPS_PER_READ`).
    BadRun {
        /// First origin_seq asked for.
        first: u64,
        /// Last origin_seq asked for.
        last: u64,
    },
    /// A stored token id is not 16 bytes (the database was edited by hand); the length found.
    BadTokenId(usize),
    /// A stored `scopes` list did not decode (the database was edited by hand).
    BadScopes(postcard::Error),
    /// A stored fingerprint's `projects`/`contexts` set did not decode (edited by hand).
    BadFingerprint(postcard::Error),
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
            StoreError::BadDevice(len) => write!(f, "stored device id is {len} bytes, not 16"),
            StoreError::BadRun { first, last } => {
                write!(
                    f,
                    "run {first}..={last} is empty, inverted or wider than one read"
                )
            }
            StoreError::BadHash(file) => write!(f, "stored hash for {file} is not 32 bytes"),
            StoreError::BadTokenId(len) => write!(f, "stored token id is {len} bytes, not 16"),
            StoreError::BadScopes(source) => write!(f, "decode stored token scopes: {source}"),
            StoreError::BadFingerprint(source) => {
                write!(f, "decode stored fingerprint project/context set: {source}")
            }
        }
    }
}

impl std::error::Error for StoreError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            StoreError::Sqlite { source, .. } => Some(source),
            StoreError::Codec { source, .. } => Some(source),
            StoreError::BadScopes(source) => Some(source),
            StoreError::BadFingerprint(source) => Some(source),
            StoreError::SchemaTooNew { .. }
            | StoreError::EmptyBatch
            | StoreError::BatchTooLarge(_)
            | StoreError::BadPath(_)
            | StoreError::BadDevice(_)
            | StoreError::BadRun { .. }
            | StoreError::ProjectionTooLarge(_)
            | StoreError::BadHash(_)
            | StoreError::BadTokenId(_) => None,
        }
    }
}
