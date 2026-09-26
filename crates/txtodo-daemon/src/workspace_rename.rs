//! `WorkspaceRename` (task workspace-vanity-name): sets or clears the name every paired device
//! shows for a workspace, as the `name` line of its `txtodo.toml` (`workspace_name.rs`).
//! Device-level like `WorkspaceRemove`: the request names the workspace by id, never a selector.
//!
//! The edit goes through the file's notes actor (`layout_sync.rs`), so it is an op like any
//! notes.md edit and reaches every paired device holding the workspace, a mirror included. It is up
//! to two ops, the old line out and then the new one in (`workspace_name::rename_steps`), so two
//! renames made at once on two devices leave two whole lines, never one name spliced into the
//! other, and every device reads the same, last, one.

use std::sync::PoisonError;

use tonic::{Request, Response, Status};
use txtodo_model::{FilePath, Principal};
use txtodo_proto::v1::{self as pb, workspace_selector::Selector};

use crate::convert::status_of;
use crate::global_service::{GlobalService, parse_workspace_id, workspace_info};
use crate::layout_file::LAYOUT_FILE;
use crate::workspace::Workspace;
use crate::workspace_name;

/// `WorkspaceRename`: resolves the workspace (opening it when it is not open yet), sets its name
/// and answers its `WorkspaceInfo`, which already shows the new name.
pub(crate) async fn rename(
    service: &GlobalService,
    r: Request<pb::WorkspaceRenameRequest>,
) -> Result<Response<pb::WorkspaceInfo>, Status> {
    let req = r.into_inner();
    let id = parse_workspace_id(&req.workspace_id)?;
    let name = workspace_name::clean(&req.name).map_err(Status::invalid_argument)?;
    let selector = pb::WorkspaceSelector {
        selector: Some(Selector::WorkspaceId(id.to_string())),
    };
    let ws = service.resolve(Some(&selector)).await?;
    set_name(
        &ws.read().unwrap_or_else(PoisonError::into_inner),
        name.as_deref(),
    )?;
    let catalog = service.catalog();
    let entry = catalog
        .list_registered_entries()?
        .into_iter()
        .find(|e| e.id == id)
        .ok_or_else(|| Status::not_found(format!("no workspace {id}")))?;
    Ok(Response::new(workspace_info(catalog, entry)))
}

/// Writes `name` (or no name, for `None`) into `ws`'s `txtodo.toml`, as ops from this device.
/// Refused (`FAILED_PRECONDITION`, nothing written) when the file holds its name in a form the
/// one-line edit cannot change.
pub(crate) fn set_name(ws: &Workspace, name: Option<&str>) -> Result<(), Status> {
    let path = FilePath::new(LAYOUT_FILE).map_err(|e| Status::internal(e.to_string()))?;
    let device = ws.device();
    // A hand edit the watcher has not recorded yet goes in first, so the rename keeps it.
    crate::layout_sync::record_disk(ws, Principal::External { device });
    let cell = ws.notes_actor(&path).map_err(status_of)?;
    let mut actor = cell.lock().unwrap_or_else(PoisonError::into_inner);
    let mut text = String::from_utf8_lossy(&actor.contents().0).into_owned();
    if text.is_empty() {
        // No file yet: the layout in force goes in beside the name, so the new file cannot read
        // as another layout (`layout_reload.rs` would then try to switch to it).
        text = crate::layout_rpc::layout_text(&ws.layout().get());
    }
    let steps = workspace_name::rename_steps(&text, name).map_err(Status::failed_precondition)?;
    for step in steps {
        actor
            .edit(&step, Principal::User { device })
            .map_err(status_of)?;
    }
    Ok(())
}
