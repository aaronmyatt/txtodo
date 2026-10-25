//! `ListEnsure`, `ListRename` and `ListRemove` (task `two-kinds-of-workspace`, ADR 0043): the named
//! lists under `<data dir>/lists/`. Device-level like the registry RPCs, so none carries a
//! selector; a list is named by what it shows, else by the name it was made under. The work is the
//! catalog's (`workspace_catalog_lists.rs`); these map the wire to it, on the blocking pool where it
//! opens or closes a workspace.
//! Ref: <https://docs.rs/tokio/latest/tokio/task/fn.spawn_blocking.html>

use std::sync::Arc;

use tonic::{Request, Response, Status};
use txtodo_proto::v1 as pb;

use crate::global_service::{GlobalService, workspace_info};
use crate::list_workspace::ListName;
use crate::workspace_catalog::WorkspaceCatalog;

/// `ListEnsure`.
pub(crate) async fn ensure(
    service: &GlobalService,
    r: Request<pb::ListEnsureRequest>,
) -> Result<Response<pb::ListEnsureResponse>, Status> {
    let name = ListName::parse(&r.into_inner().name).map_err(Status::invalid_argument)?;
    let catalog = Arc::clone(service.catalog());
    let (entry, created) = blocking(move || catalog.ensure_list(&name)).await?;
    let workspace = Some(workspace_info(service.catalog(), entry));
    Ok(Response::new(pb::ListEnsureResponse { workspace, created }))
}

/// `ListRename`: the `WorkspaceRename` write, after the new name is checked as a list name and
/// found free. Renaming changes what the list shows, never its id or folder.
pub(crate) async fn rename(
    service: &GlobalService,
    r: Request<pb::ListRenameRequest>,
) -> Result<Response<pb::WorkspaceInfo>, Status> {
    let req = r.into_inner();
    let new_name = ListName::parse(&req.new_name).map_err(Status::invalid_argument)?;
    let catalog = Arc::clone(service.catalog());
    let entry = blocking({
        let list = req.list.clone();
        move || find_not_default(&catalog, &list, "renamed")
    })
    .await?;
    refuse_taken(service.catalog(), &entry, &new_name)?;
    let rename = pb::WorkspaceRenameRequest {
        workspace_id: entry.id.to_string(),
        name: new_name.as_str().to_owned(),
    };
    crate::workspace_rename::rename(service, Request::new(rename)).await
}

/// `ListRemove`.
pub(crate) async fn remove(
    service: &GlobalService,
    r: Request<pb::ListRemoveRequest>,
) -> Result<Response<pb::ListRemoveResponse>, Status> {
    let list = r.into_inner().list;
    let catalog = Arc::clone(service.catalog());
    let moved_to = blocking(move || {
        let entry = find_not_default(&catalog, &list, "removed")?;
        catalog.remove_list(&entry)
    })
    .await?;
    Ok(Response::new(pb::ListRemoveResponse {
        removed: true,
        moved_to: moved_to.display().to_string(),
    }))
}

/// The list `query` means, except the default, which is a list too but is never renamed or removed.
fn find_not_default(
    catalog: &WorkspaceCatalog,
    query: &str,
    what: &str,
) -> Result<crate::workspace_registry::WorkspaceEntry, Status> {
    if query.trim().eq_ignore_ascii_case("default") {
        return Err(Status::failed_precondition(format!(
            "the default list cannot be {what}"
        )));
    }
    catalog.find_list(query)
}

/// `ALREADY_EXISTS` when another list shows `new_name` or was made under it.
fn refuse_taken(
    catalog: &WorkspaceCatalog,
    entry: &crate::workspace_registry::WorkspaceEntry,
    new_name: &ListName,
) -> Result<(), Status> {
    let taken = catalog.lists()?.into_iter().any(|(other, shown)| {
        other.id != entry.id
            && (shown.eq_ignore_ascii_case(new_name.as_str()) || other.id == new_name.id())
    });
    if taken {
        return Err(Status::already_exists(format!(
            "another list already shows or was made under {:?}",
            new_name.as_str()
        )));
    }
    Ok(())
}

/// Runs `work` on the blocking pool: it may open or close a workspace.
async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, Status> + Send + 'static,
) -> Result<T, Status> {
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|e| Status::internal(format!("list task failed: {e}")))?
}
