//! Why a [`crate::workspace::Workspace`] could not open. Split out of `workspace.rs` to keep that
//! file within its line budget, the same pattern as `crypto_error.rs`/`session_error.rs`/
//! `rotation_error.rs`/`keystore_error.rs`/`pairing_error.rs` in `txtodo-sync`.

use crate::handle::ActorError;
use crate::walker::{WALK_MAX_FILES, WalkError};
use std::fmt;
use txtodo_model::FilePath;
use txtodo_store::StoreError;
use txtodo_sync::{DEVICE_STATIC_KEY_BYTES, KeyStoreError};

/// Why the workspace could not open.
#[derive(Debug)]
pub enum WorkspaceError {
    /// The store failed.
    Store(StoreError),
    /// Discovery failed.
    Walk(WalkError),
    /// An actor failed to open.
    Actor(FilePath, Box<ActorError>),
    /// More documents than `WALK_MAX_FILES`.
    TooMany(usize),
    /// The sync keystore backend could not be resolved or read/written (plan M4 `sync-keystore`).
    KeyStore(KeyStoreError),
    /// A stored device static secret is not `DEVICE_STATIC_KEY_BYTES` long (the keystore was
    /// edited or corrupted by hand); the length found.
    CorruptDeviceStatic(usize),
}

impl fmt::Display for WorkspaceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WorkspaceError::Store(e) => write!(f, "store: {e}"),
            WorkspaceError::Walk(e) => write!(f, "discover documents: {e}"),
            WorkspaceError::Actor(p, e) => write!(f, "open {p}: {e}"),
            WorkspaceError::TooMany(n) => write!(f, "{n} documents, max {WALK_MAX_FILES}"),
            WorkspaceError::KeyStore(e) => write!(f, "sync keystore: {e}"),
            WorkspaceError::CorruptDeviceStatic(len) => write!(
                f,
                "stored device static secret is {len} bytes, not {DEVICE_STATIC_KEY_BYTES}"
            ),
        }
    }
}

impl std::error::Error for WorkspaceError {}

impl From<StoreError> for WorkspaceError {
    fn from(e: StoreError) -> WorkspaceError {
        WorkspaceError::Store(e)
    }
}
impl From<WalkError> for WorkspaceError {
    fn from(e: WalkError) -> WorkspaceError {
        WorkspaceError::Walk(e)
    }
}
impl From<KeyStoreError> for WorkspaceError {
    fn from(e: KeyStoreError) -> WorkspaceError {
        WorkspaceError::KeyStore(e)
    }
}
