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

use crate::server::TxtodoService;
use crate::workspace_catalog::WorkspaceCatalog;
use std::path::Path;
use std::sync::Arc;
use tonic::{Request, Response, Status};
use tracing::Instrument;
use txtodo_proto::v1::txtodo_server::Txtodo;
use txtodo_proto::v1::{self as pb};

// Re-exported (not just `use`d) so `crate::global_service::rpc_span`/`to_workspace_info` — the
// paths `pairing_grpc.rs`/`workspace_offer_grpc.rs` already call them by — keep resolving after
// this split; `parse_workspace_id` has no outside caller, so a plain `use` is enough for it.
pub(crate) use crate::global_service_helpers::{
    HasWorkspace, parse_workspace_id, rpc_span, to_workspace_info, totals_only, with_totals,
};

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

    /// `resolve`, then the per-call `TxtodoService` scoped to that workspace and its `rpc` span:
    /// the three lines every workspace-scoped handler below would otherwise repeat.
    async fn scoped<T: HasWorkspace>(
        &self,
        method: &'static str,
        r: &Request<T>,
    ) -> Result<(TxtodoService, tracing::Span), Status> {
        let ws = self.resolve(r.get_ref().workspace()).await?;
        let span = rpc_span(method, &ws);
        Ok((TxtodoService::new(ws), span))
    }

    /// `WorkspaceCatalog::resolve` for an async handler. An already-open workspace or a selector-
    /// less call answers at once; anything else may have to open or wait for a workspace, which is
    /// blocking work, so it runs on the blocking pool and never ties up a runtime worker — one
    /// slow open must not stall calls on workspaces that are ready (task `daemon-early-bind`).
    pub(crate) async fn resolve(
        &self,
        selector: Option<&pb::WorkspaceSelector>,
    ) -> Result<crate::server::SharedWorkspace, Status> {
        if let Some(ready) = self.catalog.resolve_without_waiting(selector) {
            return ready;
        }
        let catalog = Arc::clone(&self.catalog);
        let selector = selector.cloned();
        tokio::task::spawn_blocking(move || catalog.resolve(selector.as_ref()))
            .await
            .map_err(|e| Status::internal(format!("resolve task failed: {e}")))?
    }
}

#[tonic::async_trait]
impl Txtodo for GlobalService {
    async fn list_files(
        &self,
        r: Request<pb::ListFilesRequest>,
    ) -> Result<Response<pb::ListFilesResponse>, Status> {
        let (svc, span) = self.scoped("list_files", &r).await?;
        svc.list_files(r).instrument(span).await
    }

    async fn get_file(
        &self,
        r: Request<pb::GetFileRequest>,
    ) -> Result<Response<pb::FileContents>, Status> {
        let (svc, span) = self.scoped("get_file", &r).await?;
        svc.get_file(r).instrument(span).await
    }

    type WatchStream = <TxtodoService as Txtodo>::WatchStream;

    async fn watch(
        &self,
        r: Request<pb::WatchRequest>,
    ) -> Result<Response<Self::WatchStream>, Status> {
        let (svc, span) = self.scoped("watch", &r).await?;
        svc.watch(r).instrument(span).await
    }

    async fn apply(
        &self,
        r: Request<pb::ApplyRequest>,
    ) -> Result<Response<pb::ApplyResponse>, Status> {
        let (svc, span) = self.scoped("apply", &r).await?;
        svc.apply(r).instrument(span).await
    }

    async fn history(
        &self,
        r: Request<pb::HistoryRequest>,
    ) -> Result<Response<pb::HistoryResponse>, Status> {
        let (svc, span) = self.scoped("history", &r).await?;
        svc.history(r).instrument(span).await
    }

    async fn undo(
        &self,
        r: Request<pb::UndoRequest>,
    ) -> Result<Response<pb::ApplyResponse>, Status> {
        let (svc, span) = self.scoped("undo", &r).await?;
        svc.undo(r).instrument(span).await
    }

    async fn checkout(
        &self,
        r: Request<pb::CheckoutRequest>,
    ) -> Result<Response<pb::FileContents>, Status> {
        let (svc, span) = self.scoped("checkout", &r).await?;
        svc.checkout(r).instrument(span).await
    }

    async fn list_conflicts(
        &self,
        r: Request<pb::ConflictsRequest>,
    ) -> Result<Response<pb::ConflictsResponse>, Status> {
        let (svc, span) = self.scoped("list_conflicts", &r).await?;
        svc.list_conflicts(r).instrument(span).await
    }

    async fn resolve_conflict(
        &self,
        r: Request<pb::ResolveRequest>,
    ) -> Result<Response<pb::ApplyResponse>, Status> {
        let (svc, span) = self.scoped("resolve_conflict", &r).await?;
        svc.resolve_conflict(r).instrument(span).await
    }

    async fn health(
        &self,
        r: Request<pb::HealthRequest>,
    ) -> Result<Response<pb::HealthResponse>, Status> {
        // Health never waits on a workspace that is still opening: a selector-less call while any
        // open is pending answers with the totals alone. A named workspace is resolved (and so
        // promoted and waited for) like any other call.
        let totals = self.catalog.load_totals();
        if r.get_ref().workspace.is_none() && self.catalog.load_pending() > 0 {
            return Ok(Response::new(totals_only(totals)));
        }
        let (svc, span) = self.scoped("health", &r).await?;
        let resp = svc.health(r).instrument(span).await?;
        Ok(Response::new(with_totals(resp.into_inner(), totals)))
    }

    async fn get_notes(
        &self,
        r: Request<pb::GetNotesRequest>,
    ) -> Result<Response<pb::NotesDoc>, Status> {
        let (svc, span) = self.scoped("get_notes", &r).await?;
        svc.get_notes(r).instrument(span).await
    }

    async fn edit_notes(
        &self,
        r: Request<pb::NotesEditRequest>,
    ) -> Result<Response<pb::ApplyResponse>, Status> {
        let (svc, span) = self.scoped("edit_notes", &r).await?;
        svc.edit_notes(r).instrument(span).await
    }

    async fn ref_dir(
        &self,
        r: Request<pb::RefDirRequest>,
    ) -> Result<Response<pb::RefDirInfo>, Status> {
        let (svc, span) = self.scoped("ref_dir", &r).await?;
        svc.ref_dir(r).instrument(span).await
    }

    async fn prune_orphans(
        &self,
        r: Request<pb::PruneOrphansRequest>,
    ) -> Result<Response<pb::PruneOrphansResponse>, Status> {
        let (svc, span) = self.scoped("prune_orphans", &r).await?;
        svc.prune_orphans(r).instrument(span).await
    }

    async fn pair_offer(
        &self,
        r: Request<pb::PairOfferRequest>,
    ) -> Result<Response<pb::PairOfferResponse>, Status> {
        let (svc, span) = self.scoped("pair_offer", &r).await?;
        svc.pair_offer(r).instrument(span).await
    }

    async fn pair_accept(
        &self,
        r: Request<pb::PairAcceptRequest>,
    ) -> Result<Response<pb::PairResult>, Status> {
        // Id adoption (task pairing-workspace-identity) split into pairing_grpc.rs, this file's budget.
        crate::pairing_grpc::pair_accept_with_catalog(self, r).await
    }

    async fn pair_confirm_sas(
        &self,
        r: Request<pb::PairConfirmRequest>,
    ) -> Result<Response<pb::PairResult>, Status> {
        let (svc, span) = self.scoped("pair_confirm_sas", &r).await?;
        svc.pair_confirm_sas(r).instrument(span).await
    }

    async fn pair_await_peer(
        &self,
        r: Request<pb::PairAwaitPeerRequest>,
    ) -> Result<Response<pb::PairResult>, Status> {
        let (svc, span) = self.scoped("pair_await_peer", &r).await?;
        svc.pair_await_peer(r).instrument(span).await
    }

    async fn token_create(
        &self,
        r: Request<pb::TokenCreateRequest>,
    ) -> Result<Response<pb::Token>, Status> {
        let (svc, span) = self.scoped("token_create", &r).await?;
        svc.token_create(r).instrument(span).await
    }

    async fn token_list(
        &self,
        r: Request<pb::TokenListRequest>,
    ) -> Result<Response<pb::TokenListResponse>, Status> {
        let (svc, span) = self.scoped("token_list", &r).await?;
        svc.token_list(r).instrument(span).await
    }

    async fn token_revoke(
        &self,
        r: Request<pb::TokenRevokeRequest>,
    ) -> Result<Response<pb::TokenRevokeResponse>, Status> {
        let (svc, span) = self.scoped("token_revoke", &r).await?;
        svc.token_revoke(r).instrument(span).await
    }

    type OpLogStreamStream = <TxtodoService as Txtodo>::OpLogStreamStream;

    async fn op_log_stream(
        &self,
        r: Request<pb::OpLogRequest>,
    ) -> Result<Response<Self::OpLogStreamStream>, Status> {
        let (svc, span) = self.scoped("op_log_stream", &r).await?;
        svc.op_log_stream(r).instrument(span).await
    }

    async fn device_list(
        &self,
        r: Request<pb::DeviceListRequest>,
    ) -> Result<Response<pb::DeviceListResponse>, Status> {
        let (svc, span) = self.scoped("device_list", &r).await?;
        svc.device_list(r).instrument(span).await
    }

    async fn device_remove(
        &self,
        r: Request<pb::DeviceRemoveRequest>,
    ) -> Result<Response<pb::DeviceRemoveResponse>, Status> {
        let (svc, span) = self.scoped("device_remove", &r).await?;
        svc.device_remove(r).instrument(span).await
    }

    async fn migrate_identity(
        &self,
        r: Request<pb::MigrateIdentityRequest>,
    ) -> Result<Response<pb::MigrateIdentityResponse>, Status> {
        let (svc, span) = self.scoped("migrate_identity", &r).await?;
        svc.migrate_identity(r).instrument(span).await
    }

    async fn lint(
        &self,
        r: Request<pb::LintRequest>,
    ) -> Result<Response<pb::LintResponse>, Status> {
        let (svc, span) = self.scoped("lint", &r).await?;
        svc.lint(r).instrument(span).await
    }

    async fn sync_status(
        &self,
        r: Request<pb::SyncStatusRequest>,
    ) -> Result<Response<pb::SyncStatusResponse>, Status> {
        let (svc, span) = self.scoped("sync_status", &r).await?;
        svc.sync_status(r).instrument(span).await
    }

    async fn debug_set_group_key(
        &self,
        r: Request<pb::DebugSetGroupKeyRequest>,
    ) -> Result<Response<pb::DebugSetGroupKeyResponse>, Status> {
        let (svc, span) = self.scoped("debug_set_group_key", &r).await?;
        svc.debug_set_group_key(r).instrument(span).await
    }

    type BundleExportStream = <TxtodoService as Txtodo>::BundleExportStream;

    async fn bundle_export(
        &self,
        r: Request<pb::BundleExportRequest>,
    ) -> Result<Response<Self::BundleExportStream>, Status> {
        let (svc, span) = self.scoped("bundle_export", &r).await?;
        svc.bundle_export(r).instrument(span).await
    }

    async fn bundle_import(
        &self,
        r: Request<tonic::Streaming<pb::BundleChunk>>,
    ) -> Result<Response<pb::BundleImportResponse>, Status> {
        let selector = crate::bundle_grpc::workspace_selector_from_metadata(&r)?;
        let ws = self.resolve(selector.as_ref()).await?;
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
        let state = self.catalog.load_state(entry.id);
        Ok(Response::new(to_workspace_info(entry, state)))
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
            .map(|entry| {
                let state = self.catalog.load_state(entry.id);
                to_workspace_info(entry, state)
            })
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
