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
use txtodo_store::WorkspaceId;

use crate::list_workspace::ListName;
use crate::rejoin_backup;
use crate::workspace_catalog::WorkspaceCatalog;
use crate::workspace_catalog_mirror::create_list_dir;
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
        create_list_dir(&dir)
            .map_err(|e| Status::internal(format!("create {}: {e}", dir.display())))?;
        crate::join_target::require_empty(
            &dir,
            &format!("cannot make list {}", name.as_str()),
            "Move that folder away, then make the list again.",
        )?;
        let entry = self.adopt_list(id, &dir)?;
        if let Err(e) = self.ensure_open(id, &entry.root) {
            tracing::warn!(workspace_id = %id, error = %e, "list_open_failed");
        }
        Ok((entry, true))
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

    /// The list `query` means: the one that shows that name, else the one made under it (its
    /// folder's name). Case does not matter. Two lists that show the same name (two devices
    /// renamed at once) are refused as ambiguous; the folder name still reaches either.
    pub(crate) fn find_list(&self, query: &str) -> Result<WorkspaceEntry, Status> {
        let wanted = query.trim().to_ascii_lowercase();
        let lists = self.lists()?;
        let shown: Vec<_> = lists
            .iter()
            .filter(|(_, shown)| shown.to_ascii_lowercase() == wanted)
            .collect();
        if let [(entry, _)] = shown.as_slice() {
            return Ok(entry.clone());
        }
        if shown.len() > 1 {
            return Err(Status::failed_precondition(format!(
                "{} lists show the name {wanted:?}; name one by the name it was made under",
                shown.len()
            )));
        }
        lists
            .into_iter()
            .map(|(entry, _)| entry)
            .find(|e| folder_name(&e.root) == wanted)
            .ok_or_else(|| Status::not_found(format!("no list named {wanted:?} on this device")))
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

/// A list folder's name, lowercase: the name it was made under.
fn folder_name(root: &Path) -> String {
    root.file_name()
        .map(|n| n.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default()
}
