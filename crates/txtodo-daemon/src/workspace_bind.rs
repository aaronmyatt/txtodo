//! Binding a Remote mirror into a folder (task spike-txtodo-should-merge-remote-workspa, decided
//! 2026-10-17). Two clones of one repo on two devices are two workspaces: each device minted its
//! own id for its folder, so the second device's clone never sees the list the first one syncs to
//! it as a Remote mirror. A bind makes the mirror live in the clone. The folder's own copy, and the
//! workspace registered there, move aside as for a rejoin (`rejoin_backup.rs`: never deleted,
//! never sent); the mirror's `.txtodo/` and documents move in; the mirror's registry row points at
//! the folder. Same store and op log, so nothing is re-sent and no peer has to be on.
//!
//! Explicit only: two unrelated folders can share a name, and a bind moves the folder's files.

use std::path::{Path, PathBuf};
use std::sync::PoisonError;

use tonic::Status;
use txtodo_store::WorkspaceId;

use crate::rejoin_backup::{self, Plan};
use crate::workspace_catalog::WorkspaceCatalog;
use crate::workspace_load::OpenTicket;
use crate::workspace_registry::WorkspaceEntry;

/// What a bind did, or for a dry run would do.
#[derive(Debug)]
pub struct Bound {
    /// The workspace, as the registry has it afterwards (for a dry run, as it is now).
    pub entry: WorkspaceEntry,
    /// The new folder beside the folder that holds its own copy; `None` when it had none.
    pub backup: Option<PathBuf>,
    /// What moved out of the folder (or would), root-relative.
    pub moved_aside: Vec<String>,
    /// What moved in from the mirror's folder (or would), root-relative.
    pub moved_in: Vec<String>,
    /// The workspace that was registered at the folder, now un-registered.
    pub replaced: Option<WorkspaceId>,
}

/// A bind's checked inputs.
struct Target {
    mirror: WorkspaceEntry,
    folder: PathBuf,
    replaced: Option<WorkspaceEntry>,
}

/// The two moves: the folder's copy out (`aside`), the mirror's in (`incoming`, whose `backup` is
/// the folder).
struct Moves {
    aside: Plan,
    incoming: Plan,
}

impl Moves {
    fn has_aside(&self) -> bool {
        !self.aside.entries().is_empty()
    }
}

impl WorkspaceCatalog {
    /// `WorkspaceBind`: see the module doc. Refused, with nothing changed, unless `id` is a Remote
    /// mirror; for a folder that is the default's, overlaps another workspace, sits in the mirror
    /// folder or is on another disk. Blocks until the workspace is open at the folder.
    pub fn bind(&self, id: WorkspaceId, folder: &Path, dry_run: bool) -> Result<Bound, Status> {
        let target = self.bind_target(id, folder)?;
        let moves = moves_for(&target, self.clock.now_ms())?;
        if !dry_run {
            self.bind_now(&target, &moves)?;
            log_bound(&target, &moves);
        }
        let entry = match dry_run {
            true => target.mirror.clone(),
            false => self.entry_after(target.mirror.clone()),
        };
        Ok(Bound {
            entry,
            backup: moves.has_aside().then(|| moves.aside.backup.clone()),
            moved_aside: moves.aside.entries(),
            moved_in: moves.incoming.entries(),
            replaced: target.replaced.as_ref().map(|r| r.id),
        })
    }

    fn bind_target(&self, id: WorkspaceId, folder: &Path) -> Result<Target, Status> {
        let entries = self.list_registered_entries()?;
        let mirror = entries
            .iter()
            .find(|e| e.id == id)
            .cloned()
            .ok_or_else(|| Status::not_found(format!("no registered workspace {id}")))?;
        if !self.is_remote_root(&mirror.root) {
            return Err(refused(format!(
                "workspace {id} at {} is not a Remote mirror; only a workspace another device \
                 offered can move into a folder",
                mirror.root.display()
            )));
        }
        if !mirror.root_exists {
            let root = mirror.root.display();
            return Err(refused(format!("its mirror folder {root} is gone")));
        }
        // Canonical, like the registry's roots. Ref: https://doc.rust-lang.org/std/fs/fn.canonicalize.html
        let folder = folder
            .canonicalize()
            .map_err(|e| refused(format!("{}: {e}", folder.display())))?;
        self.check_folder(&mirror, &folder, &entries)?;
        let replaced = entries.iter().find(|e| e.root == folder).cloned();
        let default = self.registered_default();
        if replaced.as_ref().is_some_and(|r| Some(r.id) == default) {
            return Err(refused(
                "that folder holds the default workspace, which merges with your own devices \
                 under one reserved id"
                    .to_owned(),
            ));
        }
        Ok(Target {
            mirror,
            folder,
            replaced,
        })
    }

    /// The folder is a folder, not inside the mirrors' folder, shares no lists with a workspace
    /// other than the mirror and the one registered at exactly that root, and is on the mirror's
    /// disk (a bind renames, never copies).
    fn check_folder(
        &self,
        mirror: &WorkspaceEntry,
        folder: &Path,
        entries: &[WorkspaceEntry],
    ) -> Result<(), Status> {
        let shown = folder.display();
        if !folder.is_dir() {
            return Err(refused(format!("{shown} is not a folder")));
        }
        let in_mirrors = self
            .remote_root
            .get()
            .is_some_and(|remote| folder.starts_with(remote));
        if in_mirrors {
            return Err(refused(format!(
                "{shown} is inside txtodo's folder for mirrors"
            )));
        }
        let overlap = entries.iter().find(|other| {
            other.id != mirror.id
                && other.root != folder
                && txtodo_workspace_paths::root_overlap(folder, &other.root).is_some()
        });
        if let Some(other) = overlap {
            return Err(refused(format!(
                "{shown} shares lists with workspace {} at {}; remove the nested one first \
                 (`txtodo doctor` lists it)",
                other.id,
                other.root.display()
            )));
        }
        same_disk(&mirror.root, folder)
    }

    /// Holds both load slots, swaps the copies, repoints the registry and opens the mirror at the
    /// folder. On a refusal before the registry changes, both workspaces reopen where they were.
    fn bind_now(&self, t: &Target, moves: &Moves) -> Result<(), Status> {
        let (mirror_ticket, replaced_ticket) = self.hold_both(t)?;
        let swapped = self
            .close_both(t)
            .and_then(|()| swap(&t.folder, moves))
            .and_then(|()| self.repoint(t).map_err(|e| unswap(moves, e)));
        if let Err(e) = swapped {
            self.reopen_both(t, mirror_ticket, replaced_ticket);
            return Err(e);
        }
        let opened = self.run_open(&t.folder, t.mirror.id);
        mirror_ticket.finish(opened.clone().map_err(|s| s.message().to_owned()));
        if let (Some(ticket), Some(replaced)) = (replaced_ticket, &t.replaced) {
            ticket.finish(Err("replaced by a bind".to_owned()));
            self.slots.forget(replaced.id);
        }
        clear_mirror_folder(&moves.incoming);
        opened.map_err(|e| {
            Status::internal(format!(
                "the mirror's copy is in {}, but it did not open there: {}",
                t.folder.display(),
                e.message()
            ))
        })
    }

    fn hold_both(&self, t: &Target) -> Result<(OpenTicket, Option<OpenTicket>), Status> {
        let busy = |id| Status::unavailable(format!("workspace {id} is opening; try again"));
        let mirror = self
            .slots
            .hold(t.mirror.id)
            .ok_or_else(|| busy(t.mirror.id))?;
        let Some(replaced) = &t.replaced else {
            return Ok((mirror, None));
        };
        match self.slots.hold(replaced.id) {
            Some(ticket) => Ok((mirror, Some(ticket))),
            None => {
                // Nothing closed yet: the mirror is still open, so its slot goes back to Ready.
                mirror.finish(Ok(()));
                Err(busy(replaced.id))
            }
        }
    }

    fn close_both(&self, t: &Target) -> Result<(), Status> {
        self.close_quietly(t.mirror.id)?;
        match &t.replaced {
            Some(replaced) => self.close_quietly(replaced.id),
            None => Ok(()),
        }
    }

    /// The registry half: the folder's own workspace goes, the mirror's row takes the folder.
    fn repoint(&self, t: &Target) -> Result<(), Status> {
        let id = t.mirror.id;
        let mut registry = self.registry.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(replaced) = &t.replaced {
            registry
                .remove(replaced.id, self.clock.as_ref())
                .map_err(|e| Status::internal(format!("remove workspace {}: {e}", replaced.id)))?;
        }
        let moved = registry
            .set_root(id, &t.folder)
            .map_err(|e| Status::internal(format!("point workspace {id} at the folder: {e}")))?;
        moved
            .then_some(())
            .ok_or_else(|| Status::internal(format!("workspace {id} vanished while binding")))
    }

    /// After a refusal: each workspace opens again at its old root (files are back there).
    fn reopen_both(&self, t: &Target, mirror: OpenTicket, replaced: Option<OpenTicket>) {
        let opened = self.run_open(&t.mirror.root, t.mirror.id);
        mirror.finish(opened.map_err(|s| s.message().to_owned()));
        if let (Some(ticket), Some(entry)) = (replaced, &t.replaced) {
            let opened = self.run_open(&entry.root, entry.id);
            ticket.finish(opened.map_err(|s| s.message().to_owned()));
        }
    }
}

/// What the folder would move aside and what the mirror would move in, right now. Reads only.
fn moves_for(t: &Target, now_ms: u64) -> Result<Moves, Status> {
    let aside = rejoin_backup::plan(&t.folder, now_ms).map_err(refused)?;
    let mirror = rejoin_backup::plan(&t.mirror.root, now_ms).map_err(refused)?;
    let incoming = Plan {
        backup: t.folder.clone(),
        ..mirror
    };
    Ok(Moves { aside, incoming })
}

/// The file half: the folder's copy out, then the mirror's in. A failure moves back what moved.
fn swap(folder: &Path, moves: &Moves) -> Result<(), Status> {
    if moves.has_aside() {
        rejoin_backup::move_aside(&moves.aside).map_err(refused)?;
    }
    let undo = |e: Status| undo_aside(moves, e);
    crate::join_target::require_empty(
        folder,
        "cannot bind",
        "Something wrote to it while its copy moved; everything moved back. Try again.",
    )
    .map_err(undo)?;
    rejoin_backup::remove_guard(folder).map_err(|e| undo(Status::internal(e)))?;
    rejoin_backup::move_into(&moves.incoming).map_err(|e| undo(refused(e)))
}

/// Puts both copies back after the registry refused: the mirror's out of the folder, then the
/// folder's own in again.
fn unswap(moves: &Moves, e: Status) -> Status {
    match rejoin_backup::move_back(&moves.incoming, &moves.incoming.entries()) {
        Ok(()) => undo_aside(moves, e),
        Err(stuck) => Status::internal(format!("{}; and moving back failed: {stuck}", e.message())),
    }
}

fn undo_aside(moves: &Moves, e: Status) -> Status {
    if !moves.has_aside() {
        return e;
    }
    match rejoin_backup::move_back(&moves.aside, &moves.aside.entries()) {
        Ok(()) => e,
        Err(stuck) => Status::internal(format!("{}; and moving back failed: {stuck}", e.message())),
    }
}

/// The mirror's folder after its copy moved out: its guard goes, then every folder left empty,
/// the mirror's own included. Best effort; anything else in it stays.
/// Ref: https://doc.rust-lang.org/std/fs/fn.remove_dir.html
fn clear_mirror_folder(incoming: &Plan) {
    let _ = rejoin_backup::remove_guard(&incoming.root);
    let mut dirs: Vec<PathBuf> = incoming
        .documents
        .iter()
        .flat_map(|rel| Path::new(rel).ancestors().skip(1))
        .filter(|dir| !dir.as_os_str().is_empty())
        .map(|dir| incoming.root.join(dir))
        .collect();
    dirs.sort_by_key(|dir| std::cmp::Reverse(dir.components().count()));
    dirs.dedup();
    for dir in dirs.iter().chain(std::iter::once(&incoming.root)) {
        let _ = std::fs::remove_dir(dir);
    }
}

/// Refused unless both paths are on one filesystem: a rename across disks fails half way.
/// Ref: https://doc.rust-lang.org/std/os/unix/fs/trait.MetadataExt.html#tymethod.dev
#[cfg(unix)]
fn same_disk(mirror: &Path, folder: &Path) -> Result<(), Status> {
    use std::os::unix::fs::MetadataExt;
    let dev = |p: &Path| std::fs::metadata(p).map(|m| m.dev());
    match (dev(mirror), dev(folder)) {
        (Ok(a), Ok(b)) if a == b => Ok(()),
        (Ok(_), Ok(_)) => Err(refused(format!(
            "{} is on another disk than the mirror at {}; a bind moves files by renaming them",
            folder.display(),
            mirror.display()
        ))),
        (Err(e), _) | (_, Err(e)) => Err(refused(format!("cannot tell which disk: {e}"))),
    }
}

/// No device numbers here: a rename across disks fails, and everything moves back.
#[cfg(not(unix))]
fn same_disk(_mirror: &Path, _folder: &Path) -> Result<(), Status> {
    Ok(())
}

fn log_bound(t: &Target, moves: &Moves) {
    let (aside, incoming) = (moves.aside.entries().len(), moves.incoming.entries().len());
    let replaced = t.replaced.as_ref().map(|r| r.id.to_string());
    tracing::info!(workspace_id = %t.mirror.id, ?replaced, aside, incoming, "workspace_bound");
}

fn refused(message: String) -> Status {
    Status::failed_precondition(format!("cannot bind: {message}"))
}
