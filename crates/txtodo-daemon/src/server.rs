//! The gRPC service on the unix socket (ADR 0006). Handlers are thin: parse the request into
//! typed values (`convert.rs`), send one message to the right actor, map the reply. No file or
//! store access here except History. Ref: <https://docs.rs/tonic/latest/tonic/transport/server/>.
//!
//! No `rpc{method,workspace}` span lives here (root todo.txt `logging-daemon-datapath`): every
//! production RPC goes through `GlobalService` (`global_service.rs`), whose span already wraps the
//! call into these methods; whitebox tests that build a `TxtodoService` directly (`serve::serve`)
//! skip the catalog, and a second span here would only double-count.

use crate::convert::{
    file_kind_of, parse_mutation, parse_path, parse_principal, parse_resolution, parse_task_ref,
    to_flag,
};
use crate::handle::{ActorHandle, WATCH_CAP};
use crate::workspace::Workspace;
use std::pin::Pin;
use std::sync::{Arc, RwLock};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Request, Response, Status};
use txtodo_model::Principal;
use txtodo_proto::v1::txtodo_server::Txtodo;
use txtodo_proto::v1::{self as pb};

/// Concurrent RPCs per connection.
pub const MAX_INFLIGHT_RPCS: usize = 64;

/// The workspace behind a lock: the watcher task registers new documents, RPCs read.
pub type SharedWorkspace = Arc<RwLock<Workspace>>;

/// The service. `Clone` is a cheap `Arc` clone (`SharedWorkspace`), used to hand a live handle to
/// `forward_changes`'s spawned tasks (plan M5's `Watch` progress, `tree.rs`).
#[derive(Clone)]
pub struct TxtodoService {
    ws: SharedWorkspace,
}

impl TxtodoService {
    /// Wraps a workspace.
    pub fn new(ws: SharedWorkspace) -> TxtodoService {
        TxtodoService { ws }
    }

    // pub(crate): sibling modules (pairing_grpc, tokens, activity, progress) read the workspace.
    pub(crate) fn workspace(&self) -> std::sync::RwLockReadGuard<'_, Workspace> {
        self.ws
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// The raw `SharedWorkspace`, for a spawned task (`pairing_lan::spawn_joiner`).
    pub(crate) fn shared_workspace(&self) -> SharedWorkspace {
        Arc::clone(&self.ws)
    }
}

// `actor`/`actor_by_path`/`all_actors` live in `server_actors.rs`, `progress_for` in progress.rs,
// `status_of`/`applied_of` in convert.rs, `forward_changes` in watch_forward.rs — split out for
// the line budget.
use crate::convert::{applied_of, status_of};
use crate::watch_forward::forward_changes;

#[tonic::async_trait]
impl Txtodo for TxtodoService {
    async fn list_files(
        &self,
        _r: Request<pb::ListFilesRequest>,
    ) -> Result<Response<pb::ListFilesResponse>, Status> {
        let handles = self.all_actors();
        let mut files = Vec::with_capacity(handles.len());
        for h in handles {
            let c = h.get().await.map_err(status_of)?;
            let kind = file_kind_of(h.path());
            let progress = match kind {
                pb::FileKind::Todo => Some(self.progress_for(&h).await?),
                _ => None,
            };
            files.push(pb::FileInfo {
                path: h.path().to_string(),
                hash: c.hash.to_vec(),
                kind: kind as i32,
                progress,
            });
        }
        let tree = self.workspace_tree().await?;
        Ok(Response::new(pb::ListFilesResponse {
            tree: Some(crate::tree::to_pb_tree(&tree, &files)),
            files,
        }))
    }

    async fn get_file(
        &self,
        r: Request<pb::GetFileRequest>,
    ) -> Result<Response<pb::FileContents>, Status> {
        let h = self.actor(&r.get_ref().path)?;
        let c = h.get().await.map_err(status_of)?;
        Ok(Response::new(pb::FileContents {
            path: h.path().to_string(),
            bytes: c.bytes,
            hash: c.hash.to_vec(),
        }))
    }

    type WatchStream = Pin<Box<dyn tokio_stream::Stream<Item = Result<pb::Change, Status>> + Send>>;

    async fn watch(
        &self,
        r: Request<pb::WatchRequest>,
    ) -> Result<Response<Self::WatchStream>, Status> {
        let wanted = &r.get_ref().paths;
        let handles: Vec<ActorHandle> = if wanted.is_empty() {
            self.all_actors()
        } else {
            wanted
                .iter()
                .map(|p| self.actor(p))
                .collect::<Result<_, _>>()?
        };
        let (tx, rx) = mpsc::channel(WATCH_CAP);
        for h in handles {
            let sub = h.subscribe().await.map_err(status_of)?;
            tokio::spawn(forward_changes(self.clone(), h, sub, tx.clone()));
        }
        Ok(Response::new(Box::pin(ReceiverStream::new(rx))))
    }

    async fn apply(
        &self,
        r: Request<pb::ApplyRequest>,
    ) -> Result<Response<pb::ApplyResponse>, Status> {
        let req = r.into_inner();
        let path = parse_path(&req.path)?;
        let device = self.workspace().device();
        let principal = parse_principal(req.agent, device)?;
        let mutations = req
            .mutations
            .into_iter()
            .map(parse_mutation)
            .collect::<Result<Vec<_>, _>>()?;
        let a = self.route_apply(path, mutations, principal).await?;
        Ok(Response::new(applied_of(a)))
    }

    async fn history(
        &self,
        r: Request<pb::HistoryRequest>,
    ) -> Result<Response<pb::HistoryResponse>, Status> {
        self.history_impl(r).await
    }

    async fn undo(
        &self,
        r: Request<pb::UndoRequest>,
    ) -> Result<Response<pb::ApplyResponse>, Status> {
        let req = r.get_ref();
        let h = self.actor(&req.path)?;
        let device = self.workspace().device();
        let steps = u16::try_from(req.steps).map_err(|_| Status::invalid_argument("steps"))?;
        let a = h
            .undo(steps, Principal::User { device })
            .await
            .map_err(status_of)?;
        Ok(Response::new(applied_of(a)))
    }

    async fn checkout(
        &self,
        r: Request<pb::CheckoutRequest>,
    ) -> Result<Response<pb::FileContents>, Status> {
        let req = r.get_ref();
        let h = self.actor(&req.path)?;
        let bytes = h.checkout(req.at_wall_ms).await.map_err(status_of)?;
        let hash = crate::actor::hash_of(&bytes).to_vec();
        Ok(Response::new(pb::FileContents {
            path: h.path().to_string(),
            bytes,
            hash,
        }))
    }

    async fn list_conflicts(
        &self,
        r: Request<pb::ConflictsRequest>,
    ) -> Result<Response<pb::ConflictsResponse>, Status> {
        let h = self.actor(&r.get_ref().path)?;
        let rows = h.conflicts().await.map_err(status_of)?;
        Ok(Response::new(pb::ConflictsResponse {
            flags: rows.iter().map(to_flag).collect(),
        }))
    }

    async fn resolve_conflict(
        &self,
        r: Request<pb::ResolveRequest>,
    ) -> Result<Response<pb::ApplyResponse>, Status> {
        let req = r.into_inner();
        let h = self.actor(&req.path)?;
        let task = parse_task_ref(req.task)?;
        let resolution = parse_resolution(req.resolution)?;
        let device = self.workspace().device();
        let a = h
            .resolve(task, resolution, Principal::User { device })
            .await
            .map_err(status_of)?;
        Ok(Response::new(applied_of(a)))
    }

    async fn health(
        &self,
        r: Request<pb::HealthRequest>,
    ) -> Result<Response<pb::HealthResponse>, Status> {
        self.health_impl(r).await
    }

    async fn get_notes(
        &self,
        r: Request<pb::GetNotesRequest>,
    ) -> Result<Response<pb::NotesDoc>, Status> {
        self.get_notes_impl(r).await
    }

    async fn edit_notes(
        &self,
        r: Request<pb::NotesEditRequest>,
    ) -> Result<Response<pb::ApplyResponse>, Status> {
        self.edit_notes_impl(r).await
    }

    async fn ref_dir(
        &self,
        r: Request<pb::RefDirRequest>,
    ) -> Result<Response<pb::RefDirInfo>, Status> {
        self.ref_dir_impl(r).await
    }

    async fn prune_orphans(
        &self,
        r: Request<pb::PruneOrphansRequest>,
    ) -> Result<Response<pb::PruneOrphansResponse>, Status> {
        self.prune_orphans_impl(r).await
    }

    async fn pair_offer(
        &self,
        r: Request<pb::PairOfferRequest>,
    ) -> Result<Response<pb::PairOfferResponse>, Status> {
        self.pair_offer_impl(r).await
    }

    async fn pair_accept(
        &self,
        r: Request<pb::PairAcceptRequest>,
    ) -> Result<Response<pb::PairResult>, Status> {
        self.pair_accept_impl(r).await
    }

    async fn pair_confirm_sas(
        &self,
        r: Request<pb::PairConfirmRequest>,
    ) -> Result<Response<pb::PairResult>, Status> {
        self.pair_confirm_sas_impl(r).await
    }

    async fn pair_await_peer(
        &self,
        r: Request<pb::PairAwaitPeerRequest>,
    ) -> Result<Response<pb::PairResult>, Status> {
        self.pair_await_peer_impl(r).await
    }

    async fn token_create(
        &self,
        r: Request<pb::TokenCreateRequest>,
    ) -> Result<Response<pb::Token>, Status> {
        self.token_create_impl(r).await
    }

    async fn token_list(
        &self,
        r: Request<pb::TokenListRequest>,
    ) -> Result<Response<pb::TokenListResponse>, Status> {
        self.token_list_impl(r).await
    }

    async fn token_revoke(
        &self,
        r: Request<pb::TokenRevokeRequest>,
    ) -> Result<Response<pb::TokenRevokeResponse>, Status> {
        self.token_revoke_impl(r).await
    }

    type OpLogStreamStream = crate::activity::OpLogStream;

    async fn op_log_stream(
        &self,
        r: Request<pb::OpLogRequest>,
    ) -> Result<Response<Self::OpLogStreamStream>, Status> {
        self.op_log_stream_impl(r).await
    }

    async fn device_list(
        &self,
        r: Request<pb::DeviceListRequest>,
    ) -> Result<Response<pb::DeviceListResponse>, Status> {
        self.device_list_impl(r).await
    }

    async fn device_remove(
        &self,
        r: Request<pb::DeviceRemoveRequest>,
    ) -> Result<Response<pb::DeviceRemoveResponse>, Status> {
        self.device_remove_impl(r).await
    }

    async fn migrate_identity(
        &self,
        r: Request<pb::MigrateIdentityRequest>,
    ) -> Result<Response<pb::MigrateIdentityResponse>, Status> {
        self.migrate_identity_impl(r).await
    }

    async fn sync_status(
        &self,
        r: Request<pb::SyncStatusRequest>,
    ) -> Result<Response<pb::SyncStatusResponse>, Status> {
        self.sync_status_impl(r).await
    }

    async fn debug_set_group_key(
        &self,
        r: Request<pb::DebugSetGroupKeyRequest>,
    ) -> Result<Response<pb::DebugSetGroupKeyResponse>, Status> {
        self.debug_set_group_key_impl(r).await
    }

    type BundleExportStream = crate::bundle_grpc::BundleExportStream;

    async fn bundle_export(
        &self,
        r: Request<pb::BundleExportRequest>,
    ) -> Result<Response<Self::BundleExportStream>, Status> {
        self.bundle_export_impl(r).await
    }

    async fn bundle_import(
        &self,
        r: Request<tonic::Streaming<pb::BundleChunk>>,
    ) -> Result<Response<pb::BundleImportResponse>, Status> {
        self.bundle_import_impl(r).await
    }

    async fn workspace_add(
        &self,
        _r: Request<pb::WorkspaceAddRequest>,
    ) -> Result<Response<pb::WorkspaceInfo>, Status> {
        Err(no_registry())
    }

    async fn workspace_remove(
        &self,
        _r: Request<pb::WorkspaceRemoveRequest>,
    ) -> Result<Response<pb::WorkspaceRemoveResponse>, Status> {
        Err(no_registry())
    }

    async fn workspace_list(
        &self,
        _r: Request<pb::WorkspaceListRequest>,
    ) -> Result<Response<pb::WorkspaceListResponse>, Status> {
        Err(no_registry())
    }

    async fn workspace_pending_offers(
        &self,
        _r: Request<pb::WorkspacePendingOffersRequest>,
    ) -> Result<Response<pb::WorkspacePendingOffersResponse>, Status> {
        Err(no_registry())
    }

    async fn workspace_accept_offer(
        &self,
        _r: Request<pb::WorkspaceAcceptOfferRequest>,
    ) -> Result<Response<pb::WorkspaceInfo>, Status> {
        Err(no_registry())
    }

    async fn workspace_decline_offer(
        &self,
        _r: Request<pb::WorkspaceDeclineOfferRequest>,
    ) -> Result<Response<pb::WorkspaceDeclineOfferResponse>, Status> {
        Err(no_registry())
    }
}

/// This bare, single-workspace `TxtodoService` (whitebox tests only — production always goes
/// through `GlobalService`) has no registry to answer the workspace-management RPCs with.
pub(crate) fn no_registry() -> Status {
    Status::unimplemented("workspace management needs the global daemon's registry")
}
