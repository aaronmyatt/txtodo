//! Rejoin fresh (task sync-drift line 8): drop this device's copy of one workspace and take a
//! paired peer's. Ids already split between the two devices (a re-mint before line 1, a merge
//! before line 4) never heal on their own. A rejoin closes the workspace, moves its synced
//! documents and `.txtodo/` into a backup folder beside the root (`rejoin_backup.rs`), and opens
//! the empty folder under the same id with a fresh store. Its next session greets with no heads,
//! so it takes the peer's whole op log, this device's own ops the peer holds included, and nothing
//! from the dropped copy is ever sent.
//!
//! The registry row does not change: same id, same root, never removed and re-added, so neither
//! the mirror's "ever registered" skip nor `adopt`'s collision checks are involved. The load slot
//! is held `Loading` meanwhile: a request for the workspace waits for the fresh copy.

use std::path::{Path, PathBuf};
use std::sync::{Arc, PoisonError};
use std::time::{Duration, Instant};

use tonic::Status;
use txtodo_model::DeviceId;
use txtodo_store::WorkspaceId;

use crate::handle::{ActorHandle, ActorMsg};
use crate::rejoin_backup::{self, Plan};
use crate::server::SharedWorkspace;
use crate::workspace_catalog::WorkspaceCatalog;
use crate::workspace_registry::WorkspaceEntry;

/// "Offers it now" means an offer came within this many redial rounds (15 s each by default).
const OFFER_FRESH_ROUNDS: u32 = 4;

/// How long a closed workspace's sessions and actors get to let go of its store.
const QUIET_WAIT: Duration = Duration::from_secs(30);

/// What a rejoin did, or for a dry run would do.
#[derive(Debug)]
pub struct Rejoined {
    /// The workspace, as the registry has it afterwards.
    pub entry: WorkspaceEntry,
    /// The new folder beside the root that holds the dropped copy.
    pub backup: PathBuf,
    /// What moved (or would), root-relative: `.txtodo`, `txtodo.toml`, each document.
    pub moved: Vec<String>,
    /// Paired devices that offered this workspace lately, newest first.
    pub offering: Vec<DeviceId>,
}

impl WorkspaceCatalog {
    /// `WorkspaceRejoin`: see the module doc. Refused, with nothing changed, for the default
    /// workspace, an unknown or missing one, one that overlaps another registered workspace, or
    /// one no paired device offered lately. Blocks until the fresh copy is open.
    pub fn rejoin(&self, id: WorkspaceId, dry_run: bool) -> Result<Rejoined, Status> {
        let entry = self.rejoin_target(id)?;
        let offering = self.offered_lately(id)?;
        let plan = if dry_run {
            rejoin_backup::plan(&entry.root, self.clock.now_ms()).map_err(refused)?
        } else {
            let plan = self.rejoin_now(&entry)?;
            log_rejoined(id, &plan);
            plan
        };
        Ok(Rejoined {
            entry: self.entry_after(entry),
            backup: plan.backup.clone(),
            moved: plan.entries(),
            offering,
        })
    }

    /// The registered, non-default workspace `id`, whose folder exists and shares no lists with
    /// another registered workspace (moving its lists would pull that one's lists out from under
    /// it, and it would send the deletes on).
    fn rejoin_target(&self, id: WorkspaceId) -> Result<WorkspaceEntry, Status> {
        if self.registered_default() == Some(id) {
            return Err(Status::failed_precondition(
                "the default workspace cannot be rejoined: it merges with your own devices \
                 under one reserved id",
            ));
        }
        let entries = self.list_registered_entries()?;
        let entry = entries
            .iter()
            .find(|e| e.id == id)
            .cloned()
            .ok_or_else(|| Status::not_found(format!("no registered workspace {id}")))?;
        if !entry.root_exists {
            let root = entry.root.display();
            return Err(Status::failed_precondition(format!(
                "workspace {id}'s folder {root} is gone"
            )));
        }
        let overlap = entries.iter().find(|other| {
            other.id != id
                && txtodo_workspace_paths::root_overlap(&entry.root, &other.root).is_some()
        });
        match overlap {
            Some(other) => Err(Status::failed_precondition(format!(
                "{} shares lists with workspace {} at {}; remove the nested one first \
                 (`txtodo doctor` lists it)",
                entry.root.display(),
                other.id,
                other.root.display()
            ))),
            None => Ok(entry),
        }
    }

    /// The devices that offered `id` in the last few redial rounds; refused when none did, since
    /// nothing would then refill the emptied folder.
    fn offered_lately(&self, id: WorkspaceId) -> Result<Vec<DeviceId>, Status> {
        let round = crate::lan::resync_interval();
        let within = round * OFFER_FRESH_ROUNDS;
        let devices = self
            .open_args
            .identity
            .workspace_offers()
            .offered_by(id, within);
        if devices.is_empty() {
            return Err(Status::failed_precondition(format!(
                "no paired device offered workspace {id} in the last {} s, so nothing would \
                 refill it; rejoin while the other device is on and reachable (it offers its \
                 workspaces every {} s)",
                within.as_secs(),
                round.as_secs()
            )));
        }
        Ok(devices)
    }

    /// Holds the slot, closes the workspace, moves its copy aside and opens the folder again:
    /// empty after a move, as it was after a refusal (nothing moved, or all of it moved back).
    fn rejoin_now(&self, entry: &WorkspaceEntry) -> Result<Plan, Status> {
        let (id, root) = (entry.id, entry.root.as_path());
        let ticket = self.slots.hold(id).ok_or_else(|| {
            Status::unavailable(format!("workspace {id} is opening; try again in a moment"))
        })?;
        let moved = self.close_and_move(id, root);
        let opened = self.run_open(root, id);
        ticket.finish(opened.clone().map_err(|s| s.message().to_owned()));
        if let Err(e) = &moved {
            tracing::warn!(workspace_id = %id, error = %e.message(), "workspace_rejoin_failed");
        }
        let plan = moved?;
        opened.map_err(|e| {
            Status::internal(format!(
                "this device's copy is in {}, but the emptied folder did not open: {}",
                plan.backup.display(),
                e.message()
            ))
        })?;
        Ok(plan)
    }

    fn close_and_move(&self, id: WorkspaceId, root: &Path) -> Result<Plan, Status> {
        self.close_quietly(id)?;
        let plan = rejoin_backup::plan(root, self.clock.now_ms()).map_err(refused)?;
        rejoin_backup::move_aside(&plan).map_err(refused)?;
        let undo = |e: Status| match rejoin_backup::move_back(&plan, &plan.entries()) {
            Ok(()) => e,
            Err(stuck) => {
                Status::internal(format!("{}; and moving back failed: {stuck}", e.message()))
            }
        };
        crate::join_target::require_empty(
            root,
            "cannot rejoin",
            "Something wrote to it while its copy moved; everything moved back. Try again.",
        )
        .map_err(undo)?;
        rejoin_backup::remove_guard(root).map_err(|e| undo(Status::internal(e)))?;
        crate::workspace_catalog_mirror::create_list_dir(root)
            .map_err(|e| undo(Status::internal(format!("create todo.txt: {e}"))))?;
        Ok(plan)
    }

    /// Drops `id` from `open` (its `Drop` stops the watcher and unregisters its sync routes, which
    /// ends every live session that carried it) and waits until nothing holds its store: every
    /// actor has stopped and the database is closed. Not open at all is fine.
    fn close_quietly(&self, id: WorkspaceId) -> Result<(), Status> {
        let opened = self
            .open
            .write()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&id);
        let Some(opened) = opened else {
            return Ok(());
        };
        let ws = Arc::clone(&opened.ws);
        // Ref: https://doc.rust-lang.org/std/sync/struct.Weak.html#method.strong_count
        let store = Arc::downgrade(ws.read().unwrap_or_else(PoisonError::into_inner).store());
        drop(opened);
        stop_actors(&ws);
        drop(ws);
        let deadline = Instant::now() + QUIET_WAIT;
        while store.strong_count() > 0 {
            if Instant::now() >= deadline {
                return Err(Status::unavailable(format!(
                    "workspace {id} was still busy {} s after closing; nothing moved, and it is \
                     open again. Try again.",
                    QUIET_WAIT.as_secs()
                )));
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        Ok(())
    }

    /// `entry` as the registry has it now (`has_state` changes with a fresh store).
    fn entry_after(&self, entry: WorkspaceEntry) -> WorkspaceEntry {
        let registry = self.registry.lock().unwrap_or_else(PoisonError::into_inner);
        registry.get(entry.id).ok().flatten().unwrap_or(entry)
    }
}

/// Sends every document actor of `ws` a `Stop`. A Watch stream (`watch_forward.rs`) holds an
/// actor handle until that actor's changes end, and the actor holds the store: without this, an
/// open TUI or desktop would keep the old copy's store open for as long as it stays connected.
/// Called from a blocking thread (`WorkspaceRejoin` runs on the blocking pool).
/// Ref: https://docs.rs/tokio/latest/tokio/runtime/struct.Handle.html#method.try_current
fn stop_actors(ws: &SharedWorkspace) {
    let handles: Vec<ActorHandle> = {
        let guard = ws.read().unwrap_or_else(PoisonError::into_inner);
        let paths: Vec<_> = guard.paths().cloned().collect();
        paths
            .iter()
            .filter_map(|p| guard.actor(p).cloned())
            .collect()
    };
    let Ok(rt) = tokio::runtime::Handle::try_current() else {
        return;
    };
    for handle in handles {
        // A gone actor is fine: it has stopped already.
        let _ = rt.block_on(handle.send(ActorMsg::Stop));
    }
}

fn log_rejoined(id: WorkspaceId, plan: &Plan) {
    let (backup, moved) = (plan.backup.display(), plan.entries().len());
    tracing::info!(workspace_id = %id, %backup, moved, "workspace_rejoined");
}

fn refused(message: String) -> Status {
    Status::failed_precondition(format!("cannot rejoin: {message}"))
}
