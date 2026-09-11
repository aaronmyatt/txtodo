//! Binding the unix socket and running the tonic server until shutdown. Split from `server.rs`
//! (the service impl) for the file budget.

use crate::server::{MAX_INFLIGHT_RPCS, SharedWorkspace, TxtodoService};
use std::path::{Path, PathBuf};
use tokio_stream::wrappers::UnixListenerStream;
use txtodo_proto::v1::txtodo_server::TxtodoServer;

/// Why serving stopped early.
#[derive(Debug)]
pub enum ServeError {
    /// The socket could not be bound.
    Bind(PathBuf, std::io::Error),
    /// tonic failed.
    Transport(tonic::transport::Error),
}

impl std::fmt::Display for ServeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ServeError::Bind(p, e) => write!(f, "cannot bind socket {}: {e}", p.display()),
            ServeError::Transport(e) => write!(f, "gRPC server: {e}"),
        }
    }
}

impl std::error::Error for ServeError {}

/// Serves until `shutdown` resolves. The socket file is created here and removed on return.
pub async fn serve(
    ws: SharedWorkspace,
    socket: &Path,
    shutdown: impl Future<Output = ()>,
) -> Result<(), ServeError> {
    let listener = tokio::net::UnixListener::bind(socket)
        .map_err(|e| ServeError::Bind(socket.to_path_buf(), e))?;
    let incoming = UnixListenerStream::new(listener);
    let result = tonic::transport::Server::builder()
        .concurrency_limit_per_connection(MAX_INFLIGHT_RPCS)
        .add_service(TxtodoServer::new(TxtodoService::new(ws)))
        .serve_with_incoming_shutdown(incoming, shutdown)
        .await;
    let _removed = std::fs::remove_file(socket);
    debug_assert!(!socket.exists(), "socket file removed on shutdown");
    result.map_err(ServeError::Transport)
}
