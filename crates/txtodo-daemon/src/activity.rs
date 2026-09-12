//! Activity feed (ADR 0004 `oplog.db`): `OpLogStream` for the devices/agents screen. Same source
//! `txtodo blame` reads. Owned end-to-end by the devices-screen backend task; the RPC delegates
//! here from `server.rs` untouched.

use crate::server::TxtodoService;
use std::pin::Pin;
use tonic::{Request, Response, Status};
use txtodo_proto::v1 as pb;

/// The `OpLogStream` RPC's response stream type, shared with `server.rs`'s associated type.
pub(crate) type OpLogStream =
    Pin<Box<dyn tokio_stream::Stream<Item = Result<pb::OpLogEntry, Status>> + Send>>;

impl TxtodoService {
    /// Streams the op log newest-first for the activity pane.
    pub(crate) async fn op_log_stream_impl(
        &self,
        _r: Request<pb::OpLogRequest>,
    ) -> Result<Response<OpLogStream>, Status> {
        Err(Status::unimplemented("op_log_stream: not wired up yet"))
    }
}
