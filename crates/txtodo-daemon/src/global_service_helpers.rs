//! Free functions `global_service.rs`'s dispatch impl uses, split out purely for that file's line
//! budget (same pattern `workspace_error.rs`/`workspace_mint.rs` use out of `workspace.rs`).

use crate::server::SharedWorkspace;
use crate::workspace_catalog_load::LoadTotals;
use crate::workspace_load::LoadState;
use crate::workspace_registry::WorkspaceEntry;
use tonic::Status;
use txtodo_model::Ulid;
use txtodo_proto::v1::{self as pb};
use txtodo_store::WorkspaceId;

/// `state` is the daemon's own load state for `e` (`None` for a root it never scheduled: missing on
/// disk, or registered just now and not yet asked for).
pub(crate) fn to_workspace_info(e: WorkspaceEntry, state: Option<LoadState>) -> pb::WorkspaceInfo {
    let (load_state, load_error) = match state {
        None => (pb::WorkspaceLoadState::Unspecified, String::new()),
        Some(LoadState::Queued) => (pb::WorkspaceLoadState::Queued, String::new()),
        Some(LoadState::Loading) => (pb::WorkspaceLoadState::Loading, String::new()),
        Some(LoadState::Ready) => (pb::WorkspaceLoadState::Ready, String::new()),
        Some(LoadState::Failed(why)) => (pb::WorkspaceLoadState::Failed, why),
    };
    pb::WorkspaceInfo {
        workspace_id: e.id.to_string(),
        root: e.root.display().to_string(),
        added_at_ms: e.added_at_ms,
        root_exists: e.root_exists,
        has_state: e.has_state,
        load_state: load_state as i32,
        load_error,
        // Wired to the real values by the default-workspace and workspace-layout daemon lines.
        is_default: e.id == crate::default_workspace::default_workspace_id(),
        refs_dir: String::new(),
        todo_file: String::new(),
    }
}

/// `resp` with the device-level workspace totals filled in (task `daemon-early-bind`).
pub(crate) fn with_totals(mut resp: pb::HealthResponse, t: LoadTotals) -> pb::HealthResponse {
    resp.workspaces_registered = t.registered;
    resp.workspaces_ready = t.ready;
    resp.workspaces_loading = t.loading;
    resp.workspaces_failed = t.failed;
    resp
}

/// What `Health` answers, at once, when a selector-less call arrives while workspaces are still
/// opening: nothing about any one workspace, only that the daemon is up and the totals.
pub(crate) fn totals_only(t: LoadTotals) -> pb::HealthResponse {
    with_totals(
        pb::HealthResponse {
            version: crate::buildinfo::VERSION.to_owned(),
            release_date: crate::buildinfo::RELEASE_DATE.to_owned(),
            ..pb::HealthResponse::default()
        },
        t,
    )
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

/// A request that names a workspace: what `GlobalService::scoped` reads to route it.
pub(crate) trait HasWorkspace {
    /// The request's `WorkspaceSelector`, if it carries one.
    fn workspace(&self) -> Option<&pb::WorkspaceSelector>;
}

macro_rules! has_workspace {
    ($($request:ty),* $(,)?) => {
        $(impl HasWorkspace for $request {
            fn workspace(&self) -> Option<&pb::WorkspaceSelector> {
                self.workspace.as_ref()
            }
        })*
    };
}

has_workspace!(
    pb::ConflictsRequest,
    pb::ResolveRequest,
    pb::HealthRequest,
    pb::PruneOrphansRequest,
    pb::WorkspaceLayoutRequest,
    pb::PairConfirmRequest,
    pb::PairAwaitPeerRequest,
    pb::ApplyRequest,
    pb::BundleExportRequest,
    pb::CheckoutRequest,
    pb::DebugSetGroupKeyRequest,
    pb::DeviceListRequest,
    pb::DeviceRemoveRequest,
    pb::GetFileRequest,
    pb::GetNotesRequest,
    pb::HistoryRequest,
    pb::LintRequest,
    pb::ListFilesRequest,
    pb::MigrateIdentityRequest,
    pb::NotesEditRequest,
    pb::OpLogRequest,
    pb::PairOfferRequest,
    pb::RefDirRequest,
    pb::SyncStatusRequest,
    pb::TokenCreateRequest,
    pb::TokenListRequest,
    pb::TokenRevokeRequest,
    pb::UndoRequest,
    pb::WatchRequest
);
