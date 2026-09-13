//! Why the workspace registry could not open, add, remove or list. Split out of
//! `workspace_registry.rs` to keep that file within its line budget, the same pattern as
//! `workspace_error.rs`.

use std::fmt;
use std::path::PathBuf;
use txtodo_store::StoreError;

/// Why an operation against the workspace registry failed.
#[derive(Debug)]
pub enum WorkspaceRegistryError {
    /// The registry's own SQLite store failed.
    Store(StoreError),
    /// Could not create the registry database's parent directory.
    Io {
        /// The directory.
        path: PathBuf,
        /// The cause.
        source: std::io::Error,
    },
    /// `root` could not be canonicalized — it does not exist, or is not reachable.
    Root {
        /// The path given.
        path: PathBuf,
        /// The cause.
        source: std::io::Error,
    },
    /// `root` canonicalizes to a path that is not valid UTF-8; this crate stores paths as TEXT.
    NonUtf8Root(PathBuf),
}

impl fmt::Display for WorkspaceRegistryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WorkspaceRegistryError::Store(e) => write!(f, "registry store: {e}"),
            WorkspaceRegistryError::Io { path, source } => {
                write!(f, "create {}: {source}", path.display())
            }
            WorkspaceRegistryError::Root { path, source } => {
                write!(f, "resolve workspace root {}: {source}", path.display())
            }
            WorkspaceRegistryError::NonUtf8Root(path) => {
                write!(f, "workspace root {} is not valid UTF-8", path.display())
            }
        }
    }
}

impl std::error::Error for WorkspaceRegistryError {}

impl From<StoreError> for WorkspaceRegistryError {
    fn from(e: StoreError) -> WorkspaceRegistryError {
        WorkspaceRegistryError::Store(e)
    }
}
