//! Taking in what a paired device offers (tasks `remote-workspace-mirror`, `two-kinds-of-workspace`).
//! Only a named list is offered now (ADR 0043), and only an own device's is made, as a list of this
//! device's own (`workspace_catalog_lists.rs`); anything else a peer offers, an older build's
//! repo or default alias included, is consumed and ignored. The Remote mirror that used to land at
//! `<remote root>/<workspace-id>/` is gone; the folder survives for the mirrors an older build made
//! (`is_remote_root`), which `workspace_bind.rs` and the retire step still look at.
//!
//! The control channel only records offers (it has no catalog). `spawn_offer_mirror` waits on the
//! offer registry's wake-up and drains it here on a blocking thread, since an open blocks (store,
//! watcher, first walk).
//!
//! Skip rule: an id this registry ever held is left alone. Active covers a peer offering back one
//! of this device's own workspaces; removed means the user removed it, which is the durable "not
//! this one".

use std::path::Path;
use std::sync::{Arc, PoisonError};

use tonic::Status;
use txtodo_store::WorkspaceId;

use crate::list_workspace::is_list_id;
use crate::workspace_catalog::WorkspaceCatalog;
use crate::workspace_offer_registry::PendingOffer;

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

    /// Makes a list of every pending list offer the skip rule allows, and consumes each offer
    /// either way. Returns how many new lists it made. Blocks: call it off the async runtime.
    pub fn mirror_pending_offers(&self) -> usize {
        let offers = self.open_args.identity.workspace_offers();
        let mut made = 0;
        for offer in offers.list() {
            made += usize::from(self.mirror_offered(&offer));
            offers.take(offer.offering_device, offer.workspace_id);
        }
        made
    }

    /// One offer of [`Self::mirror_pending_offers`]: `true` when it made a new list.
    fn mirror_offered(&self, offer: &PendingOffer) -> bool {
        let id = offer.workspace_id;
        match self.try_mirror_offered(offer) {
            Ok(true) => log_mirrored(id),
            Ok(false) => false,
            Err(e) => log_mirror_failed(id, &e),
        }
    }

    fn try_mirror_offered(&self, offer: &PendingOffer) -> Result<bool, Status> {
        let id = offer.workspace_id;
        if self.ever_registered(id)? {
            return Ok(false);
        }
        // A named list is made as a list, and only for an own device (task
        // two-kinds-of-workspace). Anything else is from an older build: a repo, or a default
        // under an alias. Neither is synced by txtodo now.
        if is_list_id(id) {
            return Ok(self.mirror_offered_list(offer)?.is_some());
        }
        tracing::info!(workspace_id = %id, "workspace_offer_ignored_not_a_list");
        Ok(false)
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
