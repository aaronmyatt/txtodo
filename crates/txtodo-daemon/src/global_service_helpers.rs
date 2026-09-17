//! Free functions `global_service.rs`'s dispatch impl uses, split out purely for that file's line
//! budget (same pattern `workspace_error.rs`/`workspace_mint.rs` use out of `workspace.rs`).

use crate::server::SharedWorkspace;
use crate::workspace_registry::WorkspaceEntry;
use tonic::Status;
use txtodo_model::Ulid;
use txtodo_proto::v1::{self as pb};
use txtodo_store::WorkspaceId;

pub(crate) fn to_workspace_info(e: WorkspaceEntry) -> pb::WorkspaceInfo {
    pb::WorkspaceInfo {
        workspace_id: e.id.to_string(),
        root: e.root.display().to_string(),
        added_at_ms: e.added_at_ms,
        root_exists: e.root_exists,
        has_state: e.has_state,
    }
}

pub(crate) fn parse_workspace_id(text: &str) -> Result<WorkspaceId, Status> {
    let ulid = Ulid::parse(text)
        .ok_or_else(|| Status::invalid_argument(format!("{text:?} is not a ULID")))?;
    Ok(WorkspaceId::new(ulid))
}

/// `.instrument()`-wrapped, never `.enter()`-ed across the `.await` (shared multi-thread runtime).
/// `pub(crate)`: `pairing_grpc.rs`'s split-out `pair_accept_with_catalog` needs it too.
pub(crate) fn rpc_span(method: &'static str, ws: &SharedWorkspace) -> tracing::Span {
    let root = ws
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .root()
        .display()
        .to_string();
    tracing::info_span!("rpc", method, workspace = %root)
}
