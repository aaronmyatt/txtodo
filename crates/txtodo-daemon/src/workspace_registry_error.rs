//! Why the workspace registry could not open, add, remove or list. Split out of
//! `workspace_registry.rs` to keep that file within its line budget, the same pattern as
//! `workspace_error.rs`.

use std::fmt;
use std::path::PathBuf;
use txtodo_store::{StoreError, WorkspaceId};

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
    /// [`crate::workspace_registry::WorkspaceRegistry::adopt`] was asked to adopt `id` verbatim,
    /// but that id already names a *different* root locally — refused rather than silently
    /// repointing an existing workspace's identity at a new directory.
    IdCollision {
        /// The id an offer asked to adopt.
        id: WorkspaceId,
        /// The root that id already names locally.
        existing_root: PathBuf,
    },
    /// `adopt`'s target root is already actively registered locally under a *different* id —
    /// refused rather than creating a second catalog entry for the same directory.
    RootCollision {
        /// The root an offer asked to adopt into.
        root: PathBuf,
        /// The id that root is already registered under.
        existing_id: WorkspaceId,
    },
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
            WorkspaceRegistryError::IdCollision { id, existing_root } => write!(
                f,
                "workspace id {id} already names {} locally",
                existing_root.display()
            ),
            WorkspaceRegistryError::RootCollision { root, existing_id } => write!(
                f,
                "{} is already registered locally under a different id ({existing_id})",
                root.display()
            ),
        }
    }
}

impl std::error::Error for WorkspaceRegistryError {}

impl From<StoreError> for WorkspaceRegistryError {
    fn from(e: StoreError) -> WorkspaceRegistryError {
        WorkspaceRegistryError::Store(e)
    }
}
