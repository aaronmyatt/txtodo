//! The gRPC service on the unix socket (ADR 0006). Handlers are thin: parse the request into
//! typed values (`convert.rs`), send one message to the right actor, map the reply. No file or
//! store access happens here except the read-only History query.
//! https://docs.rs/tonic/latest/tonic/transport/server/struct.Server.html#method.serve_with_incoming

use crate::convert::{
    file_kind_of, parse_mutation, parse_path, parse_principal, parse_resolution, parse_task_ref,
    parse_ulid_opt, task_of, to_flag, to_summary,
};
use crate::handle::ConflictRow;
use crate::handle::{ActorHandle, Applied, WATCH_CAP};
use crate::workspace::Workspace;
use std::pin::Pin;
use std::sync::{Arc, RwLock};
use tokio::sync::broadcast::error::RecvError;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Request, Response, Status};
use txtodo_model::{FilePath, Principal, TaskId};
use txtodo_proto::v1::txtodo_server::Txtodo;
use txtodo_proto::v1::{self as pb};

/// History default page.
pub const HISTORY_DEFAULT_LIMIT: usize = 50;
/// History hard cap per call.
pub const HISTORY_MAX_LIMIT: usize = 1_000;
/// Concurrent RPCs per connection.
pub const MAX_INFLIGHT_RPCS: usize = 64;

/// The workspace behind a lock: the watcher task registers new documents, RPCs read.
pub type SharedWorkspace = Arc<RwLock<Workspace>>;

/// The service.
pub struct TxtodoService {
    ws: SharedWorkspace,
}

impl TxtodoService {
    /// Wraps a workspace.
    pub fn new(ws: SharedWorkspace) -> TxtodoService {
        TxtodoService { ws }
    }

    // pub(crate), not private: sibling modules (pairing_grpc.rs, tokens.rs, activity.rs, progress.rs)
    // need read access to the workspace. Every RPC body in this file's trait impl is untouched.
    pub(crate) fn workspace(&self) -> std::sync::RwLockReadGuard<'_, Workspace> {
        self.ws
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn actor(&self, path: &str) -> Result<ActorHandle, Status> {
        let path = parse_path(path)?;
        self.actor_by_path(&path)
    }

    pub(crate) fn actor_by_path(&self, path: &FilePath) -> Result<ActorHandle, Status> {
        self.workspace()
            .actor(path)
            .cloned()
            .ok_or_else(|| Status::not_found(format!("no document {path}")))
    }

    fn all_actors(&self) -> Vec<ActorHandle> {
        let ws = self.workspace();
        ws.paths().filter_map(|p| ws.actor(p).cloned()).collect()
    }
}

// `progress_for` (ListFiles progress, plan §3.2.5) lives in progress.rs, and `status_of` in
// convert.rs, both split out to keep this file within its line budget.
use crate::convert::status_of;

fn applied_of(a: Applied) -> pb::ApplyResponse {
    pb::ApplyResponse {
        applied: a.applied,
        hash: a.hash.to_vec(),
        hlc_wall_ms: a.hlc.wall_ms,
        hlc_counter: u32::from(a.hlc.counter),
    }
}

/// Forwards one actor's changes into the merged Watch stream until either side hangs up.
async fn forward_changes(
    h: ActorHandle,
    mut sub: tokio::sync::broadcast::Receiver<crate::handle::Change>,
    tx: mpsc::Sender<Result<pb::Change, Status>>,
) {
    // Bounded by the subscriber's lifetime: `tx.send` fails once the client is gone.
    loop {
        let item = match sub.recv().await {
            Ok(c) => pb::Change {
                path: c.path.to_string(),
                hash: c.hash.to_vec(),
                ops: c.ops.iter().map(to_summary).collect(),
                // Line numbers are not known on the broadcast path; ListConflicts has them.
                review: c
                    .review
                    .iter()
                    .map(|row| {
                        to_flag(&ConflictRow {
                            row: row.clone(),
                            line_number: 0,
                        })
                    })
                    .collect(),
            },
            Err(RecvError::Lagged(_)) => match h.get().await {
                Ok(c) => pb::Change {
                    path: h.path().to_string(),
                    hash: c.hash.to_vec(),
                    ops: Vec::new(),
                    review: Vec::new(),
                },
                Err(_) => return,
            },
            Err(RecvError::Closed) => return,
        };
        if tx.send(Ok(item)).await.is_err() {
            return;
        }
    }
}

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
        Ok(Response::new(pb::ListFilesResponse { files }))
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
            tokio::spawn(forward_changes(h, sub, tx.clone()));
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
        let req = r.get_ref();
        let limit = if req.limit == 0 {
            HISTORY_DEFAULT_LIMIT
        } else {
            (req.limit as usize).min(HISTORY_MAX_LIMIT)
        };
        let task = parse_ulid_opt(&req.task_id)?.map(TaskId::new);
        let paths: Vec<FilePath> = if req.path.is_empty() {
            self.workspace().paths().cloned().collect()
        } else {
            vec![parse_path(&req.path)?]
        };
        let ws = self.workspace();
        let store = ws
            .store()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut rows = Vec::new();
        for p in &paths {
            let newest = store
                .newest(p, txtodo_store::MAX_OPS_PER_READ)
                .map_err(|e| Status::internal(e.to_string()))?;
            rows.extend(
                newest
                    .into_iter()
                    .filter(|s| req.before_seq == 0 || s.seq.0 < req.before_seq),
            );
        }
        rows.retain(|s| task.is_none_or(|t| task_of(&s.op.kind) == Some(t)));
        rows.sort_by_key(|s| std::cmp::Reverse(s.seq));
        rows.truncate(limit);
        debug_assert!(rows.len() <= limit);
        Ok(Response::new(pb::HistoryResponse {
            ops: rows.iter().map(to_summary).collect(),
        }))
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
        _r: Request<pb::HealthRequest>,
    ) -> Result<Response<pb::HealthResponse>, Status> {
        let ws = self.workspace();
        let (writes_total, watcher_alive, last_event_ms) = ws.stats().read();
        let now_ms = crate::clock::Clock::now_ms(&crate::clock::SystemClock);
        let last_event_age_ms = if last_event_ms == 0 {
            u64::MAX
        } else {
            now_ms.saturating_sub(last_event_ms)
        };
        let lan = ws.lan_status();
        Ok(Response::new(pb::HealthResponse {
            watcher_alive,
            documents: u32::try_from(ws.paths().count()).unwrap_or(u32::MAX),
            last_event_age_ms,
            started_at_ms: ws.started_at_ms(),
            writes_total,
            version: env!("CARGO_PKG_VERSION").to_owned(),
            lan_relay_disabled: crate::lan_status::LanStatus::RELAY_DISABLED,
            lan_endpoint_bound: lan.endpoint_bound(),
            lan_discovery_active: lan.discovery_active(),
            lan_group_key_present: ws.has_group_key(),
        }))
    }

    async fn get_notes(&self, r: Request<pb::TaskRef>) -> Result<Response<pb::NotesDoc>, Status> {
        self.get_notes_impl(r).await
    }

    async fn edit_notes(
        &self,
        r: Request<pb::NotesEditRequest>,
    ) -> Result<Response<pb::ApplyResponse>, Status> {
        self.edit_notes_impl(r).await
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

    async fn debug_set_group_key(
        &self,
        r: Request<pb::DebugSetGroupKeyRequest>,
    ) -> Result<Response<pb::DebugSetGroupKeyResponse>, Status> {
        self.debug_set_group_key_impl(r).await
    }
}
