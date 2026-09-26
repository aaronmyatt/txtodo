//! The registry RPCs `GlobalService` delegates to (`WorkspaceAdd`/`Remove`/`List`, ADR 0025, and
//! `WorkspaceRejoin`, task sync-drift line 8), split out of `global_service.rs` for its line
//! budget, the same pattern as `workspace_offer_grpc.rs`. Device-level: none carries a selector.

use std::path::Path;
use std::sync::Arc;

use tonic::{Request, Response, Status};
use txtodo_proto::v1 as pb;

use crate::global_service::{GlobalService, parse_workspace_id, workspace_info};

pub(crate) fn add(
    service: &GlobalService,
    r: Request<pb::WorkspaceAddRequest>,
) -> Result<Response<pb::WorkspaceInfo>, Status> {
    let catalog = service.catalog();
    let entry = catalog.add_registered(Path::new(&r.into_inner().root))?;
    Ok(Response::new(workspace_info(catalog, entry)))
}

pub(crate) fn remove(
    service: &GlobalService,
    r: Request<pb::WorkspaceRemoveRequest>,
) -> Result<Response<pb::WorkspaceRemoveResponse>, Status> {
    let id = parse_workspace_id(&r.into_inner().workspace_id)?;
    let removed = service.catalog().remove_registered(id)?;
    Ok(Response::new(pb::WorkspaceRemoveResponse { removed }))
}

pub(crate) fn list(service: &GlobalService) -> Result<Response<pb::WorkspaceListResponse>, Status> {
    let catalog = service.catalog();
    let workspaces = catalog
        .list_registered_entries()?
        .into_iter()
        .map(|entry| workspace_info(catalog, entry))
        .collect();
    Ok(Response::new(pb::WorkspaceListResponse { workspaces }))
}

/// `WorkspaceRejoin`. On the blocking pool: it waits for the workspace's sessions and actors to
/// let go of its store, moves files and reopens it.
/// Ref: https://docs.rs/tokio/latest/tokio/task/fn.spawn_blocking.html
pub(crate) async fn rejoin(
    service: &GlobalService,
    r: Request<pb::WorkspaceRejoinRequest>,
) -> Result<Response<pb::WorkspaceRejoinResponse>, Status> {
    let req = r.into_inner();
    let id = parse_workspace_id(&req.workspace_id)?;
    let catalog = Arc::clone(service.catalog());
    let done = tokio::task::spawn_blocking(move || catalog.rejoin(id, req.dry_run))
        .await
        .map_err(|e| Status::internal(format!("rejoin task failed: {e}")))??;
    Ok(Response::new(pb::WorkspaceRejoinResponse {
        workspace: Some(workspace_info(service.catalog(), done.entry)),
        backup_dir: done.backup.display().to_string(),
        moved: done.moved,
        offering_devices: done.offering.iter().map(ToString::to_string).collect(),
    }))
}
