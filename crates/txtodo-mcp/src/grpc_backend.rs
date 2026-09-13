//! [`GrpcMcpBackend`]: the [`McpBackend`] implementation every transport (`transport.rs`) and the
//! CLI's `mcp` subcommand use. It is a gRPC *client* of `txtodod` — even the daemon-hosted
//! Streamable HTTP transport dials the daemon's own unix socket rather than reaching into actor
//! state directly (see the crate's As-built notes for why: it means one `McpBackend` impl, built
//! only from the RPCs `txtodo-cli`'s `client.rs` already proves out, instead of a second copy of
//! daemon-internal wiring living in this crate).

use std::path::{Path, PathBuf};

use tonic::transport::{Channel, Endpoint, Uri};
use txtodo_proto::v1 as pb;
use txtodo_proto::v1::txtodo_client::TxtodoClient;

use crate::backend::{
    ApplyOutcome, FieldPatch, FileMeta, GetTarget, Hlc, ListArgs, McpBackend, MoveAnchor,
    OpSummary, RefPath, TaskId, TaskRow, TodoOp,
};
use crate::error::McpError;
use crate::grpc_write::GrpcCtx;
use crate::{grpc_read, grpc_write};

/// Where the daemon listens, relative to the workspace dir (ADR 0010). Mirrors
/// `txtodo-cli::client::SOCKET_REL` — reimplemented here rather than imported, since
/// `budgets.json`'s `allowedDeps` lets `txtodo-mcp` depend on neither `txtodo-cli` (a binary-only
/// crate anyway) nor vice versa.
pub const SOCKET_REL: &str = ".txtodo/txtodod.sock";

/// A connected gRPC client, plus the principal to stamp on every mutation.
pub struct GrpcMcpBackend {
    ctx: GrpcCtx,
}

/// Why connecting to `txtodod` failed.
#[derive(Debug)]
pub struct ConnectError {
    /// The socket path that was dialed.
    pub socket: PathBuf,
    /// The transport error text.
    pub detail: String,
}

impl std::fmt::Display for ConnectError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "daemon socket {} refused the connection ({}); is txtodod running?",
            self.socket.display(),
            self.detail
        )
    }
}

impl std::error::Error for ConnectError {}

impl GrpcMcpBackend {
    /// Dials `txtodod`'s unix socket (the same `.txtodo/txtodod.sock` `txtodo-cli`'s `client.rs`
    /// connects to), reimplementing that small connector here since `txtodo-mcp` may not depend on
    /// the (binary-only) `txtodo-cli` crate. `agent` is attached to every mutation's
    /// `ApplyRequest.agent`; `None` until a real token principal exists
    /// ([mcp-agent-principal](../../../tasks/mcp-agent-principal)).
    #[cfg(unix)]
    pub async fn connect_unix(
        socket: &Path,
        agent: Option<(TaskId, String)>,
    ) -> Result<GrpcMcpBackend, ConnectError> {
        use hyper_util::rt::TokioIo;
        use tokio::net::UnixStream;
        use tower::service_fn;

        let dial = socket.to_path_buf();
        let channel = Endpoint::try_from("http://[::]:50051")
            .map_err(|e| ConnectError {
                socket: socket.to_path_buf(),
                detail: e.to_string(),
            })?
            .connect_with_connector(service_fn(move |_: Uri| {
                let dial = dial.clone();
                async move { UnixStream::connect(dial).await.map(TokioIo::new) }
            }))
            .await
            .map_err(|e| ConnectError {
                socket: socket.to_path_buf(),
                detail: e.to_string(),
            })?;
        Ok(GrpcMcpBackend::new(channel, agent))
    }

    /// Builds a backend from an already-connected channel (used by the above, and directly by
    /// tests / a future Streamable HTTP transport that dials over TCP instead of a unix socket).
    pub fn new(channel: Channel, agent: Option<(TaskId, String)>) -> GrpcMcpBackend {
        let agent = agent.map(|(token_id, name)| pb::AgentPrincipal { token_id, name });
        GrpcMcpBackend {
            ctx: GrpcCtx {
                client: TxtodoClient::new(channel),
                agent,
            },
        }
    }

    fn client(&self) -> TxtodoClient<Channel> {
        self.ctx.client.clone()
    }

    fn ctx(&self) -> GrpcCtx {
        self.ctx.clone()
    }
}

#[async_trait::async_trait]
impl McpBackend for GrpcMcpBackend {
    async fn list(&self, args: ListArgs) -> Result<Vec<TaskRow>, McpError> {
        grpc_read::list(self.client(), args).await
    }

    async fn search(&self, text: String, file: Option<RefPath>) -> Result<Vec<TaskRow>, McpError> {
        grpc_read::search(self.client(), text, file).await
    }

    async fn get(&self, target: GetTarget) -> Result<TaskRow, McpError> {
        grpc_read::get(self.client(), target).await
    }

    async fn add(&self, text: String, file: Option<RefPath>) -> Result<TaskRow, McpError> {
        grpc_write::add(self.ctx(), text, file).await
    }

    async fn complete(&self, id: TaskId, done: bool) -> Result<TaskRow, McpError> {
        grpc_write::complete(self.ctx(), id, done).await
    }

    async fn edit(&self, id: TaskId, patch: FieldPatch) -> Result<TaskRow, McpError> {
        grpc_write::edit(self.ctx(), id, patch).await
    }

    async fn move_task(&self, id: TaskId, anchor: MoveAnchor) -> Result<TaskRow, McpError> {
        grpc_write::move_task(self.ctx(), id, anchor).await
    }

    async fn delete(&self, id: TaskId, confirm: bool) -> Result<(), McpError> {
        grpc_write::delete(self.ctx(), id, confirm).await
    }

    async fn archive(&self, file: RefPath) -> Result<ApplyOutcome, McpError> {
        grpc_write::archive(self.ctx(), file).await
    }

    async fn batch(&self, ops: Vec<TodoOp>, dry_run: bool) -> Result<ApplyOutcome, McpError> {
        grpc_write::batch(self.ctx(), ops, dry_run).await
    }

    async fn history(
        &self,
        since: Option<Hlc>,
        id: Option<TaskId>,
        file: Option<RefPath>,
    ) -> Result<Vec<OpSummary>, McpError> {
        grpc_read::history(self.client(), since, id, file).await
    }

    async fn raw_read(&self, file: RefPath, lines: Vec<u32>) -> Result<Vec<String>, McpError> {
        grpc_read::raw_read(self.client(), file, lines).await
    }

    async fn raw_write(&self, file: RefPath, line: u32, text: String) -> Result<(), McpError> {
        grpc_write::raw_write(self.ctx(), file, line, text).await
    }

    async fn notes_get(&self, id: TaskId) -> Result<String, McpError> {
        grpc_write::notes_get(self.client(), id).await
    }

    async fn notes_set(&self, id: TaskId, text: String) -> Result<(), McpError> {
        grpc_write::notes_set(self.client(), id, text).await
    }

    async fn list_files(&self) -> Result<Vec<FileMeta>, McpError> {
        grpc_read::list_files(self.client()).await
    }

    async fn get_file(&self, file: RefPath) -> Result<String, McpError> {
        grpc_read::get_file_text(self.client(), &file).await
    }
}
