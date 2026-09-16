//! `GlobalService`: the tonic `Txtodo` impl actually wired to the one global socket
//! (ADR 0025, task `daemon-global-socket`). Every method resolves the request's `WorkspaceSelector`
//! against a `WorkspaceCatalog`, then delegates to `TxtodoService` — which keeps its existing,
//! unmodified single-workspace behaviour: every other module in this crate that reads
//! `self.workspace()`/`self.actor()`/etc. needs zero changes, since `TxtodoService::new(ws)` is
//! reconstructed fresh, per call, already scoped to the resolved workspace. `TxtodoService` itself
//! still implements `Txtodo` directly too, unchanged — kept for the whitebox tests that construct
//! one against a single already-open `Workspace` directly, bypassing the catalog entirely
//! (`serve::serve`, as opposed to this file's consumer, `serve::serve_global`). Every method also
//! wraps its delegated call in an `rpc{method,workspace}` span (`rpc_span`) — this is the one
//! place that sees every RPC, so the span lives here, not duplicated in `server.rs`.

use crate::server::{SharedWorkspace, TxtodoService};
use crate::workspace_catalog::WorkspaceCatalog;
use crate::workspace_registry::WorkspaceEntry;
use std::path::Path;
use std::sync::Arc;
use tonic::{Request, Response, Status};
use tracing::Instrument;
use txtodo_model::Ulid;
use txtodo_proto::v1::txtodo_server::Txtodo;
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

fn parse_workspace_id(text: &str) -> Result<WorkspaceId, Status> {
    let ulid = Ulid::parse(text)
        .ok_or_else(|| Status::invalid_argument(format!("{text:?} is not a ULID")))?;
    Ok(WorkspaceId::new(ulid))
}

/// `.instrument()`-wrapped, never `.enter()`-ed across the `.await` (shared multi-thread runtime).
fn rpc_span(method: &'static str, ws: &SharedWorkspace) -> tracing::Span {
    let root = ws
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .root()
        .display()
        .to_string();
    tracing::info_span!("rpc", method, workspace = %root)
}

/// The service actually bound to the one global socket. `Clone` is a cheap `Arc` clone.
#[derive(Clone)]
pub struct GlobalService {
    catalog: Arc<WorkspaceCatalog>,
}

impl GlobalService {
    /// Wraps a catalog.
    pub fn new(catalog: Arc<WorkspaceCatalog>) -> GlobalService {
        GlobalService { catalog }
    }
    /// `pub(crate)`: `workspace_offer_grpc.rs`'s own `impl Txtodo for GlobalService` extension
    /// needs the catalog too (split out for `server.rs`'s file budget).
    pub(crate) fn catalog(&self) -> &Arc<WorkspaceCatalog> {
        &self.catalog
    }
}

#[tonic::async_trait]
impl Txtodo for GlobalService {
    async fn list_files(
        &self,
        r: Request<pb::ListFilesRequest>,
    ) -> Result<Response<pb::ListFilesResponse>, Status> {
        let ws = self.catalog.resolve(r.get_ref().workspace.as_ref())?;
        let span = rpc_span("list_files", &ws);
        TxtodoService::new(ws).list_files(r).instrument(span).await
    }

    async fn get_file(
        &self,
        r: Request<pb::GetFileRequest>,
    ) -> Result<Response<pb::FileContents>, Status> {
        let ws = self.catalog.resolve(r.get_ref().workspace.as_ref())?;
        let span = rpc_span("get_file", &ws);
        TxtodoService::new(ws).get_file(r).instrument(span).await
    }

    type WatchStream = <TxtodoService as Txtodo>::WatchStream;

    async fn watch(
        &self,
        r: Request<pb::WatchRequest>,
    ) -> Result<Response<Self::WatchStream>, Status> {
        let ws = self.catalog.resolve(r.get_ref().workspace.as_ref())?;
        let span = rpc_span("watch", &ws);
        TxtodoService::new(ws).watch(r).instrument(span).await
    }

    async fn apply(
        &self,
        r: Request<pb::ApplyRequest>,
    ) -> Result<Response<pb::ApplyResponse>, Status> {
        let ws = self.catalog.resolve(r.get_ref().workspace.as_ref())?;
        let span = rpc_span("apply", &ws);
        TxtodoService::new(ws).apply(r).instrument(span).await
    }

    async fn history(
        &self,
        r: Request<pb::HistoryRequest>,
    ) -> Result<Response<pb::HistoryResponse>, Status> {
        let ws = self.catalog.resolve(r.get_ref().workspace.as_ref())?;
        let span = rpc_span("history", &ws);
        TxtodoService::new(ws).history(r).instrument(span).await
    }

    async fn undo(
        &self,
        r: Request<pb::UndoRequest>,
    ) -> Result<Response<pb::ApplyResponse>, Status> {
        let ws = self.catalog.resolve(r.get_ref().workspace.as_ref())?;
        let span = rpc_span("undo", &ws);
        TxtodoService::new(ws).undo(r).instrument(span).await
    }

    async fn checkout(
        &self,
        r: Request<pb::CheckoutRequest>,
    ) -> Result<Response<pb::FileContents>, Status> {
        let ws = self.catalog.resolve(r.get_ref().workspace.as_ref())?;
        let span = rpc_span("checkout", &ws);
        TxtodoService::new(ws).checkout(r).instrument(span).await
    }

    async fn list_conflicts(
        &self,
        r: Request<pb::ConflictsRequest>,
    ) -> Result<Response<pb::ConflictsResponse>, Status> {
        let ws = self.catalog.resolve(r.get_ref().workspace.as_ref())?;
        let span = rpc_span("list_conflicts", &ws);
        let svc = TxtodoService::new(ws);
        svc.list_conflicts(r).instrument(span).await
    }

    async fn resolve_conflict(
        &self,
        r: Request<pb::ResolveRequest>,
    ) -> Result<Response<pb::ApplyResponse>, Status> {
        let ws = self.catalog.resolve(r.get_ref().workspace.as_ref())?;
        let span = rpc_span("resolve_conflict", &ws);
        let svc = TxtodoService::new(ws);
        svc.resolve_conflict(r).instrument(span).await
    }

    async fn health(
        &self,
        r: Request<pb::HealthRequest>,
    ) -> Result<Response<pb::HealthResponse>, Status> {
        let ws = self.catalog.resolve(r.get_ref().workspace.as_ref())?;
        let span = rpc_span("health", &ws);
        TxtodoService::new(ws).health(r).instrument(span).await
    }

    async fn get_notes(
        &self,
        r: Request<pb::GetNotesRequest>,
    ) -> Result<Response<pb::NotesDoc>, Status> {
        let ws = self.catalog.resolve(r.get_ref().workspace.as_ref())?;
        let span = rpc_span("get_notes", &ws);
        TxtodoService::new(ws).get_notes(r).instrument(span).await
    }

    async fn edit_notes(
        &self,
        r: Request<pb::NotesEditRequest>,
    ) -> Result<Response<pb::ApplyResponse>, Status> {
        let ws = self.catalog.resolve(r.get_ref().workspace.as_ref())?;
        let span = rpc_span("edit_notes", &ws);
        TxtodoService::new(ws).edit_notes(r).instrument(span).await
    }

    async fn ref_dir(
        &self,
        r: Request<pb::RefDirRequest>,
    ) -> Result<Response<pb::RefDirInfo>, Status> {
        let ws = self.catalog.resolve(r.get_ref().workspace.as_ref())?;
        let span = rpc_span("ref_dir", &ws);
        TxtodoService::new(ws).ref_dir(r).instrument(span).await
    }

    async fn prune_orphans(
        &self,
        r: Request<pb::PruneOrphansRequest>,
    ) -> Result<Response<pb::PruneOrphansResponse>, Status> {
        let ws = self.catalog.resolve(r.get_ref().workspace.as_ref())?;
        let span = rpc_span("prune_orphans", &ws);
        let svc = TxtodoService::new(ws);
        svc.prune_orphans(r).instrument(span).await
    }

    async fn pair_offer(
        &self,
        r: Request<pb::PairOfferRequest>,
    ) -> Result<Response<pb::PairOfferResponse>, Status> {
        let ws = self.catalog.resolve(r.get_ref().workspace.as_ref())?;
        let span = rpc_span("pair_offer", &ws);
        TxtodoService::new(ws).pair_offer(r).instrument(span).await
    }

    async fn pair_accept(
        &self,
        r: Request<pb::PairAcceptRequest>,
    ) -> Result<Response<pb::PairResult>, Status> {
        let ws = self.catalog.resolve(r.get_ref().workspace.as_ref())?;
        let span = rpc_span("pair_accept", &ws);
        TxtodoService::new(ws).pair_accept(r).instrument(span).await
    }

    async fn pair_confirm_sas(
        &self,
        r: Request<pb::PairConfirmRequest>,
    ) -> Result<Response<pb::PairResult>, Status> {
        let ws = self.catalog.resolve(r.get_ref().workspace.as_ref())?;
        let span = rpc_span("pair_confirm_sas", &ws);
        let svc = TxtodoService::new(ws);
        svc.pair_confirm_sas(r).instrument(span).await
    }

    async fn pair_await_peer(
        &self,
        r: Request<pb::PairAwaitPeerRequest>,
    ) -> Result<Response<pb::PairResult>, Status> {
        let ws = self.catalog.resolve(r.get_ref().workspace.as_ref())?;
        let span = rpc_span("pair_await_peer", &ws);
        let svc = TxtodoService::new(ws);
        svc.pair_await_peer(r).instrument(span).await
    }

    async fn token_create(
        &self,
        r: Request<pb::TokenCreateRequest>,
    ) -> Result<Response<pb::Token>, Status> {
        let ws = self.catalog.resolve(r.get_ref().workspace.as_ref())?;
        let span = rpc_span("token_create", &ws);
        TxtodoService::new(ws)
            .token_create(r)
            .instrument(span)
            .await
    }

    async fn token_list(
        &self,
        r: Request<pb::TokenListRequest>,
    ) -> Result<Response<pb::TokenListResponse>, Status> {
        let ws = self.catalog.resolve(r.get_ref().workspace.as_ref())?;
        let span = rpc_span("token_list", &ws);
        TxtodoService::new(ws).token_list(r).instrument(span).await
    }

    async fn token_revoke(
        &self,
        r: Request<pb::TokenRevokeRequest>,
    ) -> Result<Response<pb::TokenRevokeResponse>, Status> {
        let ws = self.catalog.resolve(r.get_ref().workspace.as_ref())?;
        let span = rpc_span("token_revoke", &ws);
        TxtodoService::new(ws)
            .token_revoke(r)
            .instrument(span)
            .await
    }

    type OpLogStreamStream = <TxtodoService as Txtodo>::OpLogStreamStream;

    async fn op_log_stream(
        &self,
        r: Request<pb::OpLogRequest>,
    ) -> Result<Response<Self::OpLogStreamStream>, Status> {
        let ws = self.catalog.resolve(r.get_ref().workspace.as_ref())?;
        let span = rpc_span("op_log_stream", &ws);
        TxtodoService::new(ws)
            .op_log_stream(r)
            .instrument(span)
            .await
    }

    async fn device_list(
        &self,
        r: Request<pb::DeviceListRequest>,
    ) -> Result<Response<pb::DeviceListResponse>, Status> {
        let ws = self.catalog.resolve(r.get_ref().workspace.as_ref())?;
        let span = rpc_span("device_list", &ws);
        TxtodoService::new(ws).device_list(r).instrument(span).await
    }

    async fn device_remove(
        &self,
        r: Request<pb::DeviceRemoveRequest>,
    ) -> Result<Response<pb::DeviceRemoveResponse>, Status> {
        let ws = self.catalog.resolve(r.get_ref().workspace.as_ref())?;
        let span = rpc_span("device_remove", &ws);
        TxtodoService::new(ws)
            .device_remove(r)
            .instrument(span)
            .await
    }

    async fn debug_set_group_key(
        &self,
        r: Request<pb::DebugSetGroupKeyRequest>,
    ) -> Result<Response<pb::DebugSetGroupKeyResponse>, Status> {
        let ws = self.catalog.resolve(r.get_ref().workspace.as_ref())?;
        let span = rpc_span("debug_set_group_key", &ws);
        TxtodoService::new(ws)
            .debug_set_group_key(r)
            .instrument(span)
            .await
    }

    type BundleExportStream = <TxtodoService as Txtodo>::BundleExportStream;

    async fn bundle_export(
        &self,
        r: Request<pb::BundleExportRequest>,
    ) -> Result<Response<Self::BundleExportStream>, Status> {
        let ws = self.catalog.resolve(r.get_ref().workspace.as_ref())?;
        let span = rpc_span("bundle_export", &ws);
        TxtodoService::new(ws)
            .bundle_export(r)
            .instrument(span)
            .await
    }

    async fn bundle_import(
        &self,
        r: Request<tonic::Streaming<pb::BundleChunk>>,
    ) -> Result<Response<pb::BundleImportResponse>, Status> {
        let selector = crate::bundle_grpc::workspace_selector_from_metadata(&r)?;
        let ws = self.catalog.resolve(selector.as_ref())?;
        let span = rpc_span("bundle_import", &ws);
        TxtodoService::new(ws)
            .bundle_import(r)
            .instrument(span)
            .await
    }

    async fn workspace_add(
        &self,
        r: Request<pb::WorkspaceAddRequest>,
    ) -> Result<Response<pb::WorkspaceInfo>, Status> {
        let entry = self
            .catalog
            .add_registered(Path::new(&r.into_inner().root))?;
        Ok(Response::new(to_workspace_info(entry)))
    }

    async fn workspace_remove(
        &self,
        r: Request<pb::WorkspaceRemoveRequest>,
    ) -> Result<Response<pb::WorkspaceRemoveResponse>, Status> {
        let id = parse_workspace_id(&r.into_inner().workspace_id)?;
        let removed = self.catalog.remove_registered(id)?;
        Ok(Response::new(pb::WorkspaceRemoveResponse { removed }))
    }

    async fn workspace_list(
        &self,
        _r: Request<pb::WorkspaceListRequest>,
    ) -> Result<Response<pb::WorkspaceListResponse>, Status> {
        let workspaces = self
            .catalog
            .list_registered_entries()?
            .into_iter()
            .map(to_workspace_info)
            .collect();
        Ok(Response::new(pb::WorkspaceListResponse { workspaces }))
    }

    async fn workspace_pending_offers(
        &self,
        r: Request<pb::WorkspacePendingOffersRequest>,
    ) -> Result<Response<pb::WorkspacePendingOffersResponse>, Status> {
        crate::workspace_offer_grpc::pending_offers(self, r).await
    }

    async fn workspace_accept_offer(
        &self,
        r: Request<pb::WorkspaceAcceptOfferRequest>,
    ) -> Result<Response<pb::WorkspaceInfo>, Status> {
        crate::workspace_offer_grpc::accept_offer(self, r).await
    }

    async fn workspace_decline_offer(
        &self,
        r: Request<pb::WorkspaceDeclineOfferRequest>,
    ) -> Result<Response<pb::WorkspaceDeclineOfferResponse>, Status> {
        crate::workspace_offer_grpc::decline_offer(self, r).await
    }
}
