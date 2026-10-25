//! Named lists in the catalog (task `two-kinds-of-workspace`, ADR 0043): where they live and
//! making one. A list is a workspace under `<lists root>/<name>/` whose id its name gives
//! (`list_workspace.rs`), so every own device's `work` is the same list and needs no offer.
//!
//! Registering is the whole of making one here: sync routes a workspace once it is open, and
//! `ensure_list` opens it, as a mirror does. Same `impl WorkspaceCatalog` split as
//! `workspace_catalog_mirror.rs`.

use std::path::{Path, PathBuf};
use std::sync::PoisonError;

use tonic::Status;
use txtodo_model::DeviceId;
use txtodo_store::WorkspaceId;
use txtodo_workspace_paths::{ListRef, Pick, pick_list};

use crate::list_workspace::ListName;
use crate::rejoin_backup;
use crate::workspace_catalog::WorkspaceCatalog;
use crate::workspace_catalog_mirror::create_list_dir;
use crate::workspace_offer_registry::PendingOffer;
use crate::workspace_registry::WorkspaceEntry;

impl WorkspaceCatalog {
    /// Sets the folder lists go in, creating it. Once per catalog; a later call is ignored.
    pub fn set_lists_root(&self, dir: &Path) -> std::io::Result<()> {
        std::fs::create_dir_all(dir)?;
        // Canonical, so `is_list_root` compares like the registry's own canonical roots.
        // Ref: https://doc.rust-lang.org/std/fs/fn.canonicalize.html
        let _ = self.lists_root.set(dir.canonicalize()?);
        Ok(())
    }

    /// True when `root` is a list's folder: inside the lists root.
    pub(crate) fn is_list_root(&self, root: &Path) -> bool {
        self.lists_root
            .get()
            .is_some_and(|lists| root != lists && root.starts_with(lists))
    }

    /// Makes the list `name` when this device has none, registers it under the id its name gives
    /// and opens it so sync routes it at once; returns its entry and whether it was made now.
    /// A list that is already here comes back unchanged, unless it was renamed: `work` is then
    /// shown as something else, and joining it under the old name would be a surprise, so that is
    /// `ALREADY_EXISTS` (the message names what it shows as). A folder left in the way with text in
    /// it is refused like a mirror's (`join_target.rs`). Blocks: call it off the async runtime.
    pub(crate) fn ensure_list(&self, name: &ListName) -> Result<(WorkspaceEntry, bool), Status> {
        let lists = self
            .lists_root
            .get()
            .ok_or_else(|| Status::failed_precondition("this daemon has no folder for lists"))?;
        let (id, dir) = (name.id(), lists.join(name.as_str()));
        if let Some(entry) = self.active_entry(id)? {
            let shown = self.shown_name(&entry);
            if shown != name.as_str() {
                return Err(Status::already_exists(format!(
                    "list {name} exists on this device, shown as {shown}; use that name, or rename it",
                    name = name.as_str()
                )));
            }
            return Ok((entry, false));
        }
        if let Some((_, shown)) = self
            .lists()?
            .into_iter()
            .find(|(_, shown)| shown.eq_ignore_ascii_case(name.as_str()))
        {
            return Err(Status::already_exists(format!(
                "a list already shows the name {shown:?}; pick another, or rename that one"
            )));
        }
        let what = format!("cannot make list {}", name.as_str());
        let entry = self.make_list_at(
            id,
            &dir,
            &what,
            "Move that folder away, then make the list again.",
        )?;
        Ok((entry, true))
    }

    /// Makes the list a peer offered, when the peer is one of this user's own devices: a folder in
    /// the lists root, registered under the offered id and opened, so sync fills it. `None` for any
    /// other peer (a list is private to the user's own devices). The folder is named after the
    /// offered shown name when that is a free list name, else after the id: the receiver cannot
    /// recover the name a renamed list was made under, and does not need it, since the id carries
    /// the identity. Blocks, like [`Self::ensure_list`].
    pub(crate) fn mirror_offered_list(
        &self,
        offer: &PendingOffer,
    ) -> Result<Option<WorkspaceEntry>, Status> {
        let id = offer.workspace_id;
        if !self.is_own_device(offer.offering_device) {
            log_list_offer_ignored(id, offer.offering_device);
            return Ok(None);
        }
        let lists = self
            .lists_root
            .get()
            .ok_or_else(|| Status::failed_precondition("this daemon has no folder for lists"))?;
        let dir = lists.join(offered_folder(&offer.name, id, lists));
        let what = format!("cannot take the list {id}");
        let again = "Move that folder away; the device's next offer makes it again.";
        self.make_list_at(id, &dir, &what, again).map(Some)
    }

    /// Makes `dir` (an empty `todo.txt` in it), refuses it when text is already there, registers it
    /// under `id` and opens it so sync routes it at once. A failed open is logged, not returned: the
    /// list stays registered and the next start's loader opens it.
    fn make_list_at(
        &self,
        id: WorkspaceId,
        dir: &Path,
        what: &str,
        instead: &str,
    ) -> Result<WorkspaceEntry, Status> {
        create_list_dir(dir)
            .map_err(|e| Status::internal(format!("create {}: {e}", dir.display())))?;
        crate::join_target::require_empty(dir, what, instead)?;
        let entry = self.adopt_list(id, dir)?;
        if let Err(e) = self.ensure_open(id, &entry.root) {
            tracing::warn!(workspace_id = %id, error = %e, "list_open_failed");
        }
        Ok(entry)
    }

    /// Whether `device` is one of this user's own: a direct own pairing, or one a direct own
    /// device vouched for (ADR 0029's 2026-10-02 amendment).
    pub(crate) fn is_own_device(&self, device: DeviceId) -> bool {
        self.open_args
            .identity
            .store()
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .is_own_device(device)
            .unwrap_or(false)
    }

    /// The registry row for `id` when it is active.
    fn active_entry(&self, id: WorkspaceId) -> Result<Option<WorkspaceEntry>, Status> {
        self.registry
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(id)
            .map_err(|e| Status::internal(format!("look up workspace {id}: {e}")))
    }

    /// Registers `dir` under `id` and reads the row back.
    fn adopt_list(&self, id: WorkspaceId, dir: &Path) -> Result<WorkspaceEntry, Status> {
        let mut registry = self.registry.lock().unwrap_or_else(PoisonError::into_inner);
        let refused = |e: crate::workspace_registry_error::WorkspaceRegistryError| {
            Status::failed_precondition(format!("make list {id}: {e}"))
        };
        registry
            .adopt(id, dir, self.clock.as_ref())
            .map_err(refused)?;
        // `adopt` leaves a row removed here as it was: the list is made again under its old id.
        if registry.get(id).map_err(refused)?.is_none() {
            registry.restore(id, self.clock.as_ref()).map_err(refused)?;
        }
        registry
            .get(id)
            .map_err(|e| Status::internal(format!("look up workspace {id}: {e}")))?
            .ok_or_else(|| Status::internal(format!("workspace {id} vanished after adopting")))
    }

    /// The name a list shows: its `txtodo.toml` `name`, else its folder's.
    pub(crate) fn shown_name(&self, entry: &WorkspaceEntry) -> String {
        let offers = self.open_args.identity.workspace_offers();
        crate::workspace_name::display_name(entry.id, &entry.root, false, || {
            offers.offered_name(entry.id)
        })
    }

    /// Every list on this device, with the name each shows. The default is not among them.
    pub(crate) fn lists(&self) -> Result<Vec<(WorkspaceEntry, String)>, Status> {
        Ok(self
            .list_registered_entries()?
            .into_iter()
            .filter(|e| self.is_list_root(&e.root))
            .map(|e| {
                let shown = self.shown_name(&e);
                (e, shown)
            })
            .collect())
    }

    /// The list `query` means (`txtodo_workspace_paths::pick_list`: the one that shows that name,
    /// else the one made under it). Two lists that show the same name are refused as ambiguous; the
    /// folder name still reaches either.
    pub(crate) fn find_list(&self, query: &str) -> Result<WorkspaceEntry, Status> {
        let lists = self.lists()?;
        let refs: Vec<ListRef<'_>> = lists
            .iter()
            .map(|(entry, shown)| ListRef {
                shown,
                root: &entry.root,
            })
            .collect();
        match pick_list(&refs, query) {
            Pick::Found(i) => Ok(lists[i].0.clone()),
            Pick::Ambiguous(many) => Err(Status::failed_precondition(format!(
                "{} lists show the name {:?}; name one by the name it was made under",
                many.len(),
                query.trim()
            ))),
            Pick::NotFound => Err(Status::not_found(format!(
                "no list named {:?} on this device",
                query.trim()
            ))),
        }
    }

    /// Removes the list `entry` from this device: closes it, moves its documents and `.txtodo/`
    /// into a time-stamped folder beside it (`rejoin_backup.rs`, never deleted, never sent) and
    /// un-registers it. The other devices keep the list. Returns where the copy went. A failed
    /// move puts everything back and reopens the list. Blocks until the list lets go of its store.
    pub(crate) fn remove_list(&self, entry: &WorkspaceEntry) -> Result<PathBuf, Status> {
        let (id, root) = (entry.id, entry.root.as_path());
        let ticket = self.slots.hold(id).ok_or_else(|| {
            Status::unavailable(format!("list {id} is opening; try again in a moment"))
        })?;
        match self.close_and_move_aside(id, root) {
            Ok(backup) => {
                // The folder is empty now, bar the guard `move_aside` left.
                let _ = rejoin_backup::remove_guard(root);
                let _ = std::fs::remove_dir(root);
                let removed = self.remove_registered(id);
                ticket.finish(Err("the list was removed".to_owned()));
                removed.map(|_| backup)
            }
            Err(e) => {
                let reopened = self.run_open(root, id);
                ticket.finish(reopened.map_err(|s| s.message().to_owned()));
                Err(e)
            }
        }
    }

    fn close_and_move_aside(&self, id: WorkspaceId, root: &Path) -> Result<PathBuf, Status> {
        let refused = |why: String| Status::failed_precondition(format!("cannot remove: {why}"));
        self.close_quietly(id)?;
        let plan = rejoin_backup::plan(root, self.clock.now_ms()).map_err(refused)?;
        rejoin_backup::move_aside(&plan).map_err(refused)?;
        Ok(plan.backup)
    }
}

/// The folder name for a list taken from an offer: its shown name when that is a list name nothing
/// in the lists root uses yet, else the id in lowercase (always a safe, unique folder name).
fn offered_folder(shown: &str, id: WorkspaceId, lists: &Path) -> String {
    ListName::parse(shown)
        .ok()
        .map(|name| name.as_str().to_owned())
        .filter(|name| !lists.join(name).exists())
        .unwrap_or_else(|| id.to_string().to_ascii_lowercase())
}

fn log_list_offer_ignored(id: WorkspaceId, device: DeviceId) {
    tracing::debug!(workspace_id = %id, %device, "list_offer_ignored_not_own");
}
