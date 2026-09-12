//! Activity feed (ADR 0004 `oplog.db`): `OpLogStream` for the devices/agents screen. Same source
//! `txtodo blame` reads. Owned end-to-end by the devices-screen backend task; the RPC delegates
//! here from `server.rs` untouched.
//!
//! Not a live tail: one bounded read of the newest ops across every tracked file, newest first.
//! Nothing is fabricated — an empty log yields a stream that closes having yielded nothing.
//! `OP_LOG_STREAM_CAP` bounds the response, the way `HISTORY_MAX_LIMIT` bounds `History` and
//! `WATCH_CAP` bounds `Watch` (this crate's "every loop/stream is bounded" convention).

use crate::convert::to_summary;
use crate::server::TxtodoService;
use std::cmp::Reverse;
use std::pin::Pin;
use std::sync::PoisonError;
use tonic::{Request, Response, Status};
use txtodo_proto::v1 as pb;
use txtodo_store::MAX_OPS_PER_READ;

/// The `OpLogStream` RPC's response stream type, shared with `server.rs`'s associated type.
pub(crate) type OpLogStream =
    Pin<Box<dyn tokio_stream::Stream<Item = Result<pb::OpLogEntry, Status>> + Send>>;

/// Most entries one `OpLogStream` call returns; a hard bound like every other stream here.
pub const OP_LOG_STREAM_CAP: usize = 200;

/// The newest ops across every tracked file, newest first, at most `OP_LOG_STREAM_CAP`.
fn newest_rows(service: &TxtodoService) -> Result<Vec<txtodo_store::Stored>, Status> {
    let ws = service.workspace();
    let store = ws.store().lock().unwrap_or_else(PoisonError::into_inner);
    let mut rows = Vec::new();
    for path in ws.paths() {
        let newest = store
            .newest(path, MAX_OPS_PER_READ)
            .map_err(|e| Status::internal(e.to_string()))?;
        rows.extend(newest);
    }
    rows.sort_by_key(|s| Reverse(s.seq));
    rows.truncate(OP_LOG_STREAM_CAP);
    Ok(rows)
}

impl TxtodoService {
    /// Streams the op log newest-first for the activity pane: one bounded read across every
    /// tracked file, not a live tail (`Watch` already covers push updates for a client that wants
    /// them).
    pub(crate) async fn op_log_stream_impl(
        &self,
        _r: Request<pb::OpLogRequest>,
    ) -> Result<Response<OpLogStream>, Status> {
        let rows = newest_rows(self)?;
        let entries: Vec<pb::OpLogEntry> = rows
            .iter()
            .map(|s| {
                let summary = to_summary(s);
                pb::OpLogEntry {
                    principal: summary.principal,
                    op: summary.summary,
                    at_ms: summary.hlc_wall_ms,
                }
            })
            .collect();
        debug_assert!(entries.len() <= OP_LOG_STREAM_CAP);
        Ok(Response::new(Box::pin(tokio_stream::iter(
            entries.into_iter().map(Ok),
        ))))
    }
}
