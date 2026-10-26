//! `MirrorsRetire` (task `two-kinds-of-workspace`, ADR 0043): the Remote mirrors an older build made
//! at `<state dir>/remote/<workspace-id>/` stop syncing with the cut (a mirror is a repo-kind folder
//! now: no route, never offered), and a mirror can hold an edit that only ever reached this device
//! that way. So they are not deleted: each is closed, its synced files and `.txtodo/` move into a
//! time-stamped sibling folder (`rejoin_backup.rs`, the move a rejoin makes), and it is
//! un-registered. A removed id is never mirrored again (`ever_registered`), so an older peer's offer
//! does not bring it back.
//!
//! One mirror failing to move stops the run with that error; the ones before it are done, and a run
//! again carries on with the rest.

use std::sync::Arc;

use tonic::{Request, Response, Status};
use txtodo_proto::v1 as pb;
use txtodo_store::WorkspaceId;

use crate::global_service::GlobalService;
use crate::global_service_helpers::parse_workspace_id;
use crate::rejoin_backup::{self, Plan};
use crate::workspace_catalog::WorkspaceCatalog;
use crate::workspace_registry::WorkspaceEntry;

/// `MirrorsRetire`: runs [`WorkspaceCatalog::retire_mirrors`] on the blocking pool, since closing a
/// workspace waits for its actors to let go of the store.
/// Ref: <https://docs.rs/tokio/latest/tokio/task/fn.spawn_blocking.html>
pub(crate) async fn mirrors_retire(
    service: &GlobalService,
    r: Request<pb::MirrorsRetireRequest>,
) -> Result<Response<pb::MirrorsRetireResponse>, Status> {
    let req = r.into_inner();
    let only = (!req.workspace_id.is_empty())
        .then(|| parse_workspace_id(&req.workspace_id))
        .transpose()?;
    let catalog = Arc::clone(service.catalog());
    let mirrors = tokio::task::spawn_blocking(move || catalog.retire_mirrors(only, req.dry_run))
        .await
        .map_err(|e| Status::internal(format!("retire task failed: {e}")))??;
    Ok(Response::new(pb::MirrorsRetireResponse { mirrors }))
}

impl WorkspaceCatalog {
    /// Retires `only`, or every Remote mirror when `None`; a dry run moves and un-registers
    /// nothing. Blocks: call it off the async runtime.
    pub fn retire_mirrors(
        &self,
        only: Option<WorkspaceId>,
        dry_run: bool,
    ) -> Result<Vec<pb::RetiredMirror>, Status> {
        let mut retired = Vec::new();
        for entry in self.mirrors_to_retire(only)? {
            let plan = self.retire_plan(&entry)?;
            if !dry_run {
                self.retire_now(&entry, plan.as_ref())?;
                tracing::info!(workspace_id = %entry.id, "workspace_mirror_retired");
            }
            retired.push(pb::RetiredMirror {
                workspace_id: entry.id.to_string(),
                root: entry.root.display().to_string(),
                moved_to: plan.map_or_else(String::new, |p| p.backup.display().to_string()),
                name: self.shown_name(&entry),
            });
        }
        Ok(retired)
    }

    /// The registered Remote mirrors, or just `only`, which must be one.
    fn mirrors_to_retire(&self, only: Option<WorkspaceId>) -> Result<Vec<WorkspaceEntry>, Status> {
        let entries = self.list_registered_entries()?;
        let Some(id) = only else {
            return Ok(entries
                .into_iter()
                .filter(|e| self.is_remote_root(&e.root))
                .collect());
        };
        let entry = entries
            .into_iter()
            .find(|e| e.id == id)
            .ok_or_else(|| Status::not_found(format!("no registered workspace {id}")))?;
        if !self.is_remote_root(&entry.root) {
            return Err(Status::failed_precondition(format!(
                "workspace {id} at {} is not a Remote mirror; only a mirror an older build made is \
                 retired",
                entry.root.display()
            )));
        }
        Ok(vec![entry])
    }

    /// What moving `entry` aside would do; `None` when its folder is gone (nothing to move).
    fn retire_plan(&self, entry: &WorkspaceEntry) -> Result<Option<Plan>, Status> {
        if !entry.root_exists {
            return Ok(None);
        }
        let plan = rejoin_backup::plan(&entry.root, self.clock.now_ms());
        plan.map(Some).map_err(refused)
    }

    /// Holds the slot, closes the mirror, moves its copy aside and un-registers it. A refusal
    /// before the registry changes reopens the mirror where it was.
    fn retire_now(&self, entry: &WorkspaceEntry, plan: Option<&Plan>) -> Result<(), Status> {
        let id = entry.id;
        let ticket = self.slots.hold(id).ok_or_else(|| {
            Status::unavailable(format!("workspace {id} is opening; try again in a moment"))
        })?;
        let moved = self
            .close_quietly(id)
            .and_then(|()| plan.map_or(Ok(()), move_away));
        if let Err(e) = moved {
            let reopened = self.run_open(&entry.root, id);
            ticket.finish(reopened.map_err(|s| s.message().to_owned()));
            return Err(e);
        }
        // Slot and `open` are forgotten by the removal; the ticket only has to be settled.
        let removed = self.remove_registered(id);
        ticket.finish(Err("retired".to_owned()));
        removed.map(|_| ())
    }
}

/// Moves the mirror's files into the backup, drops the guard and the folder when it ends empty.
fn move_away(plan: &Plan) -> Result<(), Status> {
    rejoin_backup::move_aside(plan).map_err(refused)?;
    rejoin_backup::remove_guard(&plan.root).map_err(Status::internal)?;
    // `remove_dir` only removes an empty folder, so anything the move left stays.
    // Ref: https://doc.rust-lang.org/std/fs/fn.remove_dir.html
    let _ = std::fs::remove_dir(&plan.root);
    Ok(())
}

fn refused(message: String) -> Status {
    Status::failed_precondition(format!("cannot retire the mirror: {message}"))
}
