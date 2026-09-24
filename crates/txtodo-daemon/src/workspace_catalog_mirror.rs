//! Mirroring offered workspaces (task `remote-workspace-mirror`, decided 2026-09-23 and 2026-09-24):
//! every workspace a paired device offers lands on this device on its own, no prompt, at
//! `<remote root>/<workspace-id>/`, and shows as a Remote entry in every client. The peer is
//! already trusted (it holds the group key, which only pairing hands out) and the content is plain
//! text, so this is the same trust boundary sync already uses for every op.
//!
//! The control channel only records offers (it has no catalog). `spawn_offer_mirror` waits on the
//! offer registry's wake-up and drains it here on a blocking thread, since an open blocks (store,
//! watcher, first walk).
//!
//! Skip rule: an id this registry ever held is left alone. Active covers the default (ADR 0029: it
//! merges by its reserved id) and a peer offering back one of this device's own workspaces; removed
//! means the user removed the mirror, which is the durable "not this one".

use std::path::{Path, PathBuf};
use std::sync::{Arc, PoisonError};

use tonic::Status;
use txtodo_store::WorkspaceId;

use crate::workspace_catalog::WorkspaceCatalog;
use crate::workspace_registry::WorkspaceEntry;

impl WorkspaceCatalog {
    /// Sets the folder mirrors go in, creating it. Once per catalog; a later call is ignored.
    pub fn set_remote_root(&self, dir: &Path) -> std::io::Result<()> {
        std::fs::create_dir_all(dir)?;
        // Canonical, so `is_remote_root` compares like the registry's own canonical roots.
        // Ref: https://doc.rust-lang.org/std/fs/fn.canonicalize.html
        let _ = self.remote_root.set(dir.canonicalize()?);
        Ok(())
    }

    /// True when `root` is a mirror: a folder inside the remote root.
    pub(crate) fn is_remote_root(&self, root: &Path) -> bool {
        self.remote_root
            .get()
            .is_some_and(|remote| root != remote && root.starts_with(remote))
    }

    /// Mirrors every pending offer the skip rule allows, and consumes each one either way.
    /// Returns how many new mirrors it made. Blocks: call it off the async runtime.
    pub fn mirror_pending_offers(&self) -> usize {
        let offers = self.open_args.identity.workspace_offers();
        let mut mirrored = 0;
        for offer in offers.list() {
            mirrored += usize::from(self.mirror_offered(offer.workspace_id));
            offers.take(offer.offering_device, offer.workspace_id);
        }
        mirrored
    }

    /// One offer of [`Self::mirror_pending_offers`]: `true` when it made a new mirror.
    fn mirror_offered(&self, id: WorkspaceId) -> bool {
        match self.try_mirror_offered(id) {
            Ok(true) => log_mirrored(id),
            Ok(false) => false,
            Err(e) => log_mirror_failed(id, &e),
        }
    }

    fn try_mirror_offered(&self, id: WorkspaceId) -> Result<bool, Status> {
        if self.ever_registered(id)? {
            return Ok(false);
        }
        self.mirror_workspace(id)?;
        Ok(true)
    }

    /// Registers `id` at its mirror folder (made with an empty `todo.txt` when missing) and opens
    /// it, so sync routes it at once. A failed open is logged, not returned: the workspace stays
    /// registered and the next start's loader opens it.
    pub(crate) fn mirror_workspace(&self, id: WorkspaceId) -> Result<WorkspaceEntry, Status> {
        let dir = self.mirror_dir(id)?;
        create_list_dir(&dir)
            .map_err(|e| Status::internal(format!("create {}: {e}", dir.display())))?;
        let entry = {
            let mut registry = self.registry.lock().unwrap_or_else(PoisonError::into_inner);
            registry
                .adopt(id, &dir, self.clock.as_ref())
                .map_err(|e| Status::invalid_argument(format!("mirror {id}: {e}")))?;
            registry
                .get(id)
                .map_err(|e| Status::internal(format!("look up workspace {id}: {e}")))?
                .ok_or_else(|| {
                    Status::internal(format!("workspace {id} vanished after adopting"))
                })?
        };
        if let Err(e) = self.ensure_open(id, &entry.root) {
            tracing::warn!(workspace_id = %id, error = %e, "workspace_mirror_open_failed");
        }
        Ok(entry)
    }

    /// Sets the mirror folder and starts [`Self::spawn_offer_mirror`]. A folder that cannot be made
    /// is logged: offers then stay pending, nothing else is lost.
    pub fn start_offer_mirror(self: &Arc<Self>, dir: &Path) {
        match self.set_remote_root(dir) {
            Ok(()) => drop(self.spawn_offer_mirror()),
            Err(e) => log_mirror_root_unavailable(dir, &e),
        }
    }

    /// Mirrors offers as they arrive, and any already pending, until the runtime stops.
    pub fn spawn_offer_mirror(self: &Arc<Self>) -> tokio::task::JoinHandle<()> {
        let catalog = Arc::clone(self);
        tokio::spawn(async move {
            loop {
                let pass = Arc::clone(&catalog);
                // Ref: https://docs.rs/tokio/latest/tokio/task/fn.spawn_blocking.html
                if let Err(e) =
                    tokio::task::spawn_blocking(move || pass.mirror_pending_offers()).await
                {
                    tracing::warn!(error = %e, "workspace_mirror_pass_panicked");
                }
                catalog
                    .open_args
                    .identity
                    .workspace_offers()
                    .recorded()
                    .await;
            }
        })
    }

    fn ever_registered(&self, id: WorkspaceId) -> Result<bool, Status> {
        self.registry
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .ever_registered(id)
            .map_err(|e| Status::internal(format!("look up workspace {id}: {e}")))
    }

    fn mirror_dir(&self, id: WorkspaceId) -> Result<PathBuf, Status> {
        self.remote_root
            .get()
            .map(|remote| remote.join(id.to_string()))
            .ok_or_else(|| Status::failed_precondition("this daemon has no folder for mirrors"))
    }
}

/// Creates `dir` and an empty `todo.txt` in it when they are missing; never touches a `todo.txt`
/// that is already there. Shared with the default workspace, which starts the same way.
pub(crate) fn create_list_dir(dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    // `create_new` so an existing `todo.txt` is never truncated.
    // Ref: https://doc.rust-lang.org/std/fs/struct.OpenOptions.html#method.create_new
    match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(dir.join("todo.txt"))
    {
        Ok(_) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Ok(()),
        Err(e) => Err(e),
    }
}

fn log_mirror_root_unavailable(dir: &Path, e: &std::io::Error) {
    tracing::warn!(dir = %dir.display(), error = %e, "workspace_mirror_root_unavailable");
}

fn log_mirrored(id: WorkspaceId) -> bool {
    tracing::info!(workspace_id = %id, "workspace_mirrored");
    true
}

fn log_mirror_failed(id: WorkspaceId, e: &Status) -> bool {
    tracing::warn!(workspace_id = %id, error = %e, "workspace_mirror_failed");
    false
}
