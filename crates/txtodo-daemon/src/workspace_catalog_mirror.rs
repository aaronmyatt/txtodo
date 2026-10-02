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
        let own_aliases = self.own_default_aliases();
        self.drop_own_alias_mirrors(&own_aliases);
        let offers = self.open_args.identity.workspace_offers();
        let mut mirrored = 0;
        for offer in offers.list() {
            if !own_aliases.contains(&offer.workspace_id)
                && !self.is_this_devices_alias(offer.workspace_id)
            {
                mirrored += usize::from(self.mirror_offered(offer.workspace_id));
            }
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

    /// Every own device's default alias (task default-workspace-pairing-consent): this device
    /// already merges those lists under the reserved id, so none is mirrored. Whoever offers one:
    /// a peer re-offers the mirrors it holds, so a device that is not own to that one relays its
    /// alias here too (lab chaos 20261001-233439: a1 mirrored its own shared list twice). Own
    /// includes a device a direct own peer vouched for (ADR 0029's 2026-10-02 amendment).
    fn own_default_aliases(&self) -> Vec<WorkspaceId> {
        let store = self
            .open_args
            .identity
            .store()
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        // Both bounded by `MAX_DEVICES_PER_READ`.
        let direct = store
            .list_devices()
            .unwrap_or_default()
            .into_iter()
            .filter(|row| {
                row.removed_at_ms.is_none() && store.is_own_device(row.device).unwrap_or(false)
            });
        let mut own: Vec<_> = direct.map(|row| row.device).collect();
        own.extend(store.vouched_own_devices().unwrap_or_default());
        own.into_iter()
            .map(crate::default_workspace::default_alias)
            .collect()
    }

    /// A Remote mirror of a device that has since become own (a vouch arrived after its alias was
    /// mirrored): its tasks reach the default anyway, so the mirror is removed. Removing it closes
    /// its routes, so live sessions greet again. The folder stays on disk.
    fn drop_own_alias_mirrors(&self, own_aliases: &[WorkspaceId]) {
        let entries = self.list_registered_entries().unwrap_or_default();
        for entry in entries {
            if own_aliases.contains(&entry.id) && self.is_remote_root(&entry.root) {
                self.drop_mirror(entry.id);
            }
        }
    }

    fn drop_mirror(&self, id: WorkspaceId) {
        match self.remove_registered(id) {
            Ok(_) => tracing::info!(workspace_id = %id, "workspace_mirror_dropped_now_own"),
            Err(e) => log_mirror_drop_failed(id, &e),
        }
    }

    /// This device's own default, offered back under its alias by a peer that mirrors it (a peer
    /// offers every workspace it holds, mirrors too). The alias is never registered here, so the
    /// "ever held" rule misses it. Mirrored, it became a second, empty workspace answering to the
    /// alias, which could take the sync route from the real default and starve the peer's mirror
    /// (`tests/e2e/default_workspace_foreign.rs`, CI run 36299140047).
    fn is_this_devices_alias(&self, id: WorkspaceId) -> bool {
        id == crate::default_workspace::default_alias(self.open_args.identity.device())
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
    /// registered and the next start's loader opens it. A folder left there with text in it is
    /// refused (sync-drift line 4, `join_target.rs`), unless `id` is already registered.
    pub(crate) fn mirror_workspace(&self, id: WorkspaceId) -> Result<WorkspaceEntry, Status> {
        let dir = self.mirror_dir(id)?;
        create_list_dir(&dir)
            .map_err(|e| Status::internal(format!("create {}: {e}", dir.display())))?;
        if !self.is_active(id)? {
            crate::join_target::require_empty(
                &dir,
                &format!("cannot mirror workspace {id}"),
                "Move that folder away; the device's next offer mirrors it again.",
            )?;
        }
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

    /// Whether `id` is registered and not removed: already mirrored, so its folder may hold text.
    fn is_active(&self, id: WorkspaceId) -> Result<bool, Status> {
        self.registry
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(id)
            .map(|entry| entry.is_some())
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

fn log_mirror_drop_failed(id: WorkspaceId, e: &Status) {
    tracing::warn!(workspace_id = %id, error = %e, "workspace_mirror_drop_failed");
}
