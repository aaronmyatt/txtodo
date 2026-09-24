//! The default workspace (task `default-workspace`, decided 2026-09-20): every user's one managed
//! todo list, created on first run in a directory txtodo owns, and synced to their other devices
//! like any workspace.
//!
//! Identity is a **reserved `WorkspaceId`** every device registers its default under, so paired
//! devices already exchange its ops: sync frames are keyed by workspace id, and nothing is
//! negotiated. Location is never synced (`txtodo_workspace_paths::default_workspace_dir`): devices
//! agree on identity, not path. The directory starts with an empty `todo.txt`: a seed task would be
//! added once per device and come back from sync as duplicates.
//!
//! The constant is permanent. Changing it later is a migration, so it is picked once here, with a
//! non-zero ULID timestamp so it can never be the all-zero `LINK_WORKSPACE` sentinel a sync link
//! seals its own hello under.

use crate::workspace_catalog::WorkspaceCatalog;
use std::path::Path;
use tonic::Status;
use txtodo_model::Ulid;
use txtodo_store::WorkspaceId;

/// The reserved id: timestamp `0x019A_DEFA_0177` (non-zero), then a fixed tail.
const DEFAULT_WORKSPACE_ULID: u128 = 0x019A_DEFA_0177_0000_0000_0000_0000_0001;

/// The one id every device's default workspace is registered under.
pub fn default_workspace_id() -> WorkspaceId {
    WorkspaceId::new(Ulid::from_u128(DEFAULT_WORKSPACE_ULID))
}

impl WorkspaceCatalog {
    /// Creates the default workspace's directory and an empty `todo.txt` when they are missing,
    /// and registers it under the reserved id. Idempotent, and never overwrites a `todo.txt`
    /// that is already there. Opening it is the loader's job (it queues the default first).
    ///
    /// Refused, not forced, when the reserved id is already registered at a different root: the
    /// default's directory moved, which is out of scope, and silently re-pointing it could hide
    /// the tasks the old directory holds.
    pub fn ensure_default_workspace(&self, dir: &Path) -> Result<WorkspaceId, Status> {
        crate::workspace_catalog_mirror::create_list_dir(dir).map_err(|e| {
            Status::internal(format!("default workspace create {}: {e}", dir.display()))
        })?;
        let id = default_workspace_id();
        self.registry
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .adopt(id, dir, self.clock.as_ref())
            .map_err(|e| Status::failed_precondition(format!("default workspace: {e}")))?;
        // A second ensure with the same id sets the same value; a differing one cannot happen,
        // the id is a constant.
        let _ = self.default.set(id);
        Ok(id)
    }

    /// The default workspace's id when this catalog has registered one, else `None`.
    pub(crate) fn registered_default(&self) -> Option<WorkspaceId> {
        self.default.get().copied()
    }
}
