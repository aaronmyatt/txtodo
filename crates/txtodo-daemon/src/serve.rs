//! Binding the unix socket and running the tonic server until shutdown. Split from `server.rs`
//! (the service impl) for the file budget.

use crate::global_service::GlobalService;
use crate::server::{MAX_INFLIGHT_RPCS, SharedWorkspace, TxtodoService};
use crate::workspace_catalog::WorkspaceCatalog;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio_stream::wrappers::UnixListenerStream;
use txtodo_proto::v1::txtodo_server::{Txtodo, TxtodoServer};

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

/// The true readiness signal (`ref:daemon-ready-log-ordering`): only emitted once
/// `UnixListener::bind` has actually succeeded, unlike `main.rs`'s earlier "starting" log. Split
/// out of `serve_with` for its cognitive-complexity budget, same pattern as `main.rs`'s
/// `log_ready`/`log_stopped`.
fn log_socket_bound(socket: &Path) {
    tracing::info!(socket = %socket.display(), "daemon_ready");
}

/// Binds `socket`, serves `svc` until `shutdown` resolves, then removes the socket file. Shared
/// tail of [`serve`] and [`serve_global`] — one bind/serve/cleanup path, not two that could drift.
async fn serve_with<S: Txtodo>(
    svc: S,
    socket: &Path,
    shutdown: impl Future<Output = ()>,
    after_bind: impl FnOnce(),
) -> Result<(), ServeError> {
    let listener = tokio::net::UnixListener::bind(socket)
        .map_err(|e| ServeError::Bind(socket.to_path_buf(), e))?;
    log_socket_bound(socket);
    after_bind();
    let incoming = UnixListenerStream::new(listener);
    let result = tonic::transport::Server::builder()
        .concurrency_limit_per_connection(MAX_INFLIGHT_RPCS)
        .add_service(TxtodoServer::new(svc))
        .serve_with_incoming_shutdown(incoming, shutdown)
        .await;
    let _removed = std::fs::remove_file(socket);
    debug_assert!(!socket.exists(), "socket file removed on shutdown");
    result.map_err(ServeError::Transport)
}

/// Serves a single, already-open workspace until `shutdown` resolves. The socket file is created
/// here and removed on return. Kept exactly as-is for whitebox tests that construct one
/// `Workspace` directly and bypass the registry/catalog entirely; production (`main.rs`) always
/// uses [`serve_global`] instead.
pub async fn serve(
    ws: SharedWorkspace,
    socket: &Path,
    shutdown: impl Future<Output = ()>,
) -> Result<(), ServeError> {
    serve_with(TxtodoService::new(ws), socket, shutdown, || {}).await
}

/// The real production entry point (`main.rs`): the one global socket, every call routed by
/// `WorkspaceSelector` through [`GlobalService`] (ADR 0025, task `daemon-global-socket`).
pub async fn serve_global(
    catalog: Arc<WorkspaceCatalog>,
    socket: &Path,
    shutdown: impl Future<Output = ()>,
) -> Result<(), ServeError> {
    serve_global_then(catalog, socket, shutdown, || {}).await
}

/// [`serve_global`], running `after_bind` the moment the socket is bound and `daemon_ready` is
/// logged, before the first request is served: `main.rs` starts the background workspace loader
/// there, so the daemon answers before it has opened anything (task `daemon-early-bind`).
pub async fn serve_global_then(
    catalog: Arc<WorkspaceCatalog>,
    socket: &Path,
    shutdown: impl Future<Output = ()>,
    after_bind: impl FnOnce(),
) -> Result<(), ServeError> {
    serve_with(GlobalService::new(catalog), socket, shutdown, after_bind).await
}
