//! `GlobalService`: the tonic `Txtodo` impl actually wired to the one global socket
//! (ADR 0025, task `daemon-global-socket`). Every method resolves the request's `WorkspaceSelector`
//! against a `WorkspaceCatalog`, then delegates to `TxtodoService` — which keeps its existing,
//! unmodified single-workspace behaviour: every other module in this crate that reads
//! `self.workspace()`/`self.actor()`/etc. needs zero changes, since `TxtodoService::new(ws)` is
//! reconstructed fresh, per call, already scoped to the resolved workspace. `TxtodoService` itself
//! still implements `Txtodo` directly too, unchanged — kept for the whitebox tests that construct
//! one against a single already-open `Workspace` directly, bypassing the catalog entirely
//! (`serve::serve`, as opposed to this file's consumer, `serve::serve_global`).

use crate::server::TxtodoService;
use crate::workspace_catalog::WorkspaceCatalog;
use crate::workspace_registry::WorkspaceEntry;
use std::path::Path;
use std::sync::Arc;
use tonic::{Request, Response, Status};
use txtodo_model::Ulid;
use txtodo_proto::v1::txtodo_server::Txtodo;
use txtodo_proto::v1::{self as pb};
use txtodo_store::WorkspaceId;

fn to_workspace_info(e: WorkspaceEntry) -> pb::WorkspaceInfo {
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
}

#[tonic::async_trait]
impl Txtodo for GlobalService {
    async fn list_files(
        &self,
        r: Request<pb::ListFilesRequest>,
    ) -> Result<Response<pb::ListFilesResponse>, Status> {
        let ws = self.catalog.resolve(r.get_ref().workspace.as_ref())?;
        TxtodoService::new(ws).list_files(r).await
    }

    async fn get_file(
        &self,
        r: Request<pb::GetFileRequest>,
    ) -> Result<Response<pb::FileContents>, Status> {
        let ws = self.catalog.resolve(r.get_ref().workspace.as_ref())?;
        TxtodoService::new(ws).get_file(r).await
    }

    type WatchStream = <TxtodoService as Txtodo>::WatchStream;

    async fn watch(
        &self,
        r: Request<pb::WatchRequest>,
    ) -> Result<Response<Self::WatchStream>, Status> {
        let ws = self.catalog.resolve(r.get_ref().workspace.as_ref())?;
        TxtodoService::new(ws).watch(r).await
    }

    async fn apply(
        &self,
        r: Request<pb::ApplyRequest>,
    ) -> Result<Response<pb::ApplyResponse>, Status> {
        let ws = self.catalog.resolve(r.get_ref().workspace.as_ref())?;
        TxtodoService::new(ws).apply(r).await
    }

    async fn history(
        &self,
        r: Request<pb::HistoryRequest>,
    ) -> Result<Response<pb::HistoryResponse>, Status> {
        let ws = self.catalog.resolve(r.get_ref().workspace.as_ref())?;
        TxtodoService::new(ws).history(r).await
    }

    async fn undo(
        &self,
        r: Request<pb::UndoRequest>,
    ) -> Result<Response<pb::ApplyResponse>, Status> {
        let ws = self.catalog.resolve(r.get_ref().workspace.as_ref())?;
        TxtodoService::new(ws).undo(r).await
    }

    async fn checkout(
        &self,
        r: Request<pb::CheckoutRequest>,
    ) -> Result<Response<pb::FileContents>, Status> {
        let ws = self.catalog.resolve(r.get_ref().workspace.as_ref())?;
        TxtodoService::new(ws).checkout(r).await
    }

    async fn list_conflicts(
        &self,
        r: Request<pb::ConflictsRequest>,
    ) -> Result<Response<pb::ConflictsResponse>, Status> {
        let ws = self.catalog.resolve(r.get_ref().workspace.as_ref())?;
        TxtodoService::new(ws).list_conflicts(r).await
    }

    async fn resolve_conflict(
        &self,
        r: Request<pb::ResolveRequest>,
    ) -> Result<Response<pb::ApplyResponse>, Status> {
        let ws = self.catalog.resolve(r.get_ref().workspace.as_ref())?;
        TxtodoService::new(ws).resolve_conflict(r).await
    }

    async fn health(
        &self,
        r: Request<pb::HealthRequest>,
    ) -> Result<Response<pb::HealthResponse>, Status> {
        let ws = self.catalog.resolve(r.get_ref().workspace.as_ref())?;
        TxtodoService::new(ws).health(r).await
    }

    async fn get_notes(
        &self,
        r: Request<pb::GetNotesRequest>,
    ) -> Result<Response<pb::NotesDoc>, Status> {
        let ws = self.catalog.resolve(r.get_ref().workspace.as_ref())?;
        TxtodoService::new(ws).get_notes(r).await
    }

    async fn edit_notes(
        &self,
        r: Request<pb::NotesEditRequest>,
    ) -> Result<Response<pb::ApplyResponse>, Status> {
        let ws = self.catalog.resolve(r.get_ref().workspace.as_ref())?;
        TxtodoService::new(ws).edit_notes(r).await
    }

    async fn ref_dir(
        &self,
        r: Request<pb::RefDirRequest>,
    ) -> Result<Response<pb::RefDirInfo>, Status> {
        let ws = self.catalog.resolve(r.get_ref().workspace.as_ref())?;
        TxtodoService::new(ws).ref_dir(r).await
    }

    async fn prune_orphans(
        &self,
        r: Request<pb::PruneOrphansRequest>,
    ) -> Result<Response<pb::PruneOrphansResponse>, Status> {
        let ws = self.catalog.resolve(r.get_ref().workspace.as_ref())?;
        TxtodoService::new(ws).prune_orphans(r).await
    }

    async fn pair_offer(
        &self,
        r: Request<pb::PairOfferRequest>,
    ) -> Result<Response<pb::PairOfferResponse>, Status> {
        let ws = self.catalog.resolve(r.get_ref().workspace.as_ref())?;
        TxtodoService::new(ws).pair_offer(r).await
    }

    async fn pair_accept(
        &self,
        r: Request<pb::PairAcceptRequest>,
    ) -> Result<Response<pb::PairResult>, Status> {
        let ws = self.catalog.resolve(r.get_ref().workspace.as_ref())?;
        TxtodoService::new(ws).pair_accept(r).await
    }

    async fn pair_confirm_sas(
        &self,
        r: Request<pb::PairConfirmRequest>,
    ) -> Result<Response<pb::PairResult>, Status> {
        let ws = self.catalog.resolve(r.get_ref().workspace.as_ref())?;
        TxtodoService::new(ws).pair_confirm_sas(r).await
    }

    async fn pair_await_peer(
        &self,
        r: Request<pb::PairAwaitPeerRequest>,
    ) -> Result<Response<pb::PairResult>, Status> {
        let ws = self.catalog.resolve(r.get_ref().workspace.as_ref())?;
        TxtodoService::new(ws).pair_await_peer(r).await
    }

    async fn token_create(
        &self,
        r: Request<pb::TokenCreateRequest>,
    ) -> Result<Response<pb::Token>, Status> {
        let ws = self.catalog.resolve(r.get_ref().workspace.as_ref())?;
        TxtodoService::new(ws).token_create(r).await
    }

    async fn token_list(
        &self,
        r: Request<pb::TokenListRequest>,
    ) -> Result<Response<pb::TokenListResponse>, Status> {
        let ws = self.catalog.resolve(r.get_ref().workspace.as_ref())?;
        TxtodoService::new(ws).token_list(r).await
    }

    async fn token_revoke(
        &self,
        r: Request<pb::TokenRevokeRequest>,
    ) -> Result<Response<pb::TokenRevokeResponse>, Status> {
        let ws = self.catalog.resolve(r.get_ref().workspace.as_ref())?;
        TxtodoService::new(ws).token_revoke(r).await
    }

    type OpLogStreamStream = <TxtodoService as Txtodo>::OpLogStreamStream;

    async fn op_log_stream(
        &self,
        r: Request<pb::OpLogRequest>,
    ) -> Result<Response<Self::OpLogStreamStream>, Status> {
        let ws = self.catalog.resolve(r.get_ref().workspace.as_ref())?;
        TxtodoService::new(ws).op_log_stream(r).await
    }

    async fn device_list(
        &self,
        r: Request<pb::DeviceListRequest>,
    ) -> Result<Response<pb::DeviceListResponse>, Status> {
        let ws = self.catalog.resolve(r.get_ref().workspace.as_ref())?;
        TxtodoService::new(ws).device_list(r).await
    }

    async fn device_remove(
        &self,
        r: Request<pb::DeviceRemoveRequest>,
    ) -> Result<Response<pb::DeviceRemoveResponse>, Status> {
        let ws = self.catalog.resolve(r.get_ref().workspace.as_ref())?;
        TxtodoService::new(ws).device_remove(r).await
    }

    async fn debug_set_group_key(
        &self,
        r: Request<pb::DebugSetGroupKeyRequest>,
    ) -> Result<Response<pb::DebugSetGroupKeyResponse>, Status> {
        let ws = self.catalog.resolve(r.get_ref().workspace.as_ref())?;
        TxtodoService::new(ws).debug_set_group_key(r).await
    }

    type BundleExportStream = <TxtodoService as Txtodo>::BundleExportStream;

    async fn bundle_export(
        &self,
        r: Request<pb::BundleExportRequest>,
    ) -> Result<Response<Self::BundleExportStream>, Status> {
        let ws = self.catalog.resolve(r.get_ref().workspace.as_ref())?;
        TxtodoService::new(ws).bundle_export(r).await
    }

    async fn bundle_import(
        &self,
        r: Request<tonic::Streaming<pb::BundleChunk>>,
    ) -> Result<Response<pb::BundleImportResponse>, Status> {
        let selector = crate::bundle_grpc::workspace_selector_from_metadata(&r)?;
        let ws = self.catalog.resolve(selector.as_ref())?;
        TxtodoService::new(ws).bundle_import(r).await
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
}
