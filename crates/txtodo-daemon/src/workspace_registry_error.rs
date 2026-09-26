//! Why the workspace registry could not open, add, remove or list. Split out of
//! `workspace_registry.rs` to keep that file within its line budget, the same pattern as
//! `workspace_error.rs`.

use std::fmt;
use std::path::PathBuf;
use txtodo_store::{StoreError, WorkspaceId};
use txtodo_workspace_paths::RootOverlap;

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
    /// `root` is inside, or holds, a workspace already registered here, and the walk of one
    /// reaches the other's lists (`txtodo_workspace_paths::root_overlap`, sync-drift line 3):
    /// refused, since two stores would then track the same files with different ids.
    Overlap {
        /// The root asked for.
        root: PathBuf,
        /// Whether `root` is inside `registered` or holds it.
        overlap: RootOverlap,
        /// The registered root it overlaps.
        registered: PathBuf,
        /// That registered root's id.
        registered_id: WorkspaceId,
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
            WorkspaceRegistryError::Overlap {
                root,
                overlap,
                registered,
                registered_id,
            } => write_overlap(f, root, *overlap, (registered, *registered_id)),
        }
    }
}

/// The overlap refusal, naming the registered root and what to do instead.
fn write_overlap(
    f: &mut fmt::Formatter<'_>,
    root: &std::path::Path,
    overlap: RootOverlap,
    (registered, id): (&std::path::Path, WorkspaceId),
) -> fmt::Result {
    match overlap {
        RootOverlap::Inside => write!(
            f,
            "{} is inside the registered workspace {} ({id}), whose lists already include it; \
             use that workspace instead",
            root.display(),
            registered.display()
        ),
        RootOverlap::Around => write!(
            f,
            "{} holds the registered workspace {} ({id}), so both would track its lists; \
             remove that one first (`txtodo workspace remove {id}`)",
            root.display(),
            registered.display()
        ),
    }
}

impl std::error::Error for WorkspaceRegistryError {}

impl From<StoreError> for WorkspaceRegistryError {
    fn from(e: StoreError) -> WorkspaceRegistryError {
        WorkspaceRegistryError::Store(e)
    }
}
