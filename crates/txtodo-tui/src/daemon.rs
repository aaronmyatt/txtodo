//! The gRPC bridge to `txtodod`, mirroring `apps/desktop/src-tauri/src/daemon.rs`'s
//! `DaemonClient` — same ADR 0010 unix-socket transport, same lazily-dialed `tonic::Channel`,
//! deliberately not shared as a library between the two clients since desktop is thin over Tauri
//! commands and the TUI is thin over `crossterm` events; duplicating a dozen lines of transport
//! setup is cheaper than a new shared crate for the workspace boundary (allowedDeps["txtodo-tui"]
//! is `[txtodo-core, txtodo-proto]` only — no new member).
//!
//! **Known gap:** `SyncStatus` (design §7's `s` indicator RPC) does not exist on the wire yet —
//! adding it needs a `crates/txtodo-proto` schema change and a `crates/txtodo-daemon` handler,
//! both outside this crate's slice (this session's edit fence allows exactly one crate; see the
//! worktree's own report for the follow-up task). `AppState.sync` is therefore never populated
//! from a real daemon today; `ui/sync.rs` still renders whatever is in it (design §3.3: colours
//! are never the only signal, and here the signal is simply absent until that RPC lands).

use std::fmt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use txtodo_proto::v1 as pb;

/// Per-attempt bound for [`Daemon::wait_until_ready`]'s `Health` probe. Gated with the same
/// `#[cfg(unix)]` as its one use site (`connect`'s unix-socket dial): on Windows that arm is
/// `UnsupportedPlatform` and an ungated const is dead code, which CI's `-D warnings` rejects.
/// Ref: https://doc.rust-lang.org/reference/conditional-compilation.html#the-cfg-attribute
#[cfg(unix)]
const CONNECT_TIMEOUT: Duration = Duration::from_millis(500);
/// How many probe attempts [`Daemon::wait_until_ready`] makes before giving up.
const MAX_CONNECT_RETRIES: u32 = 5;
/// Delay between probe attempts.
const RETRY_BACKOFF: Duration = Duration::from_millis(150);
/// Bound on `Watch`-drop reconnect attempts (design: "never loop unbounded; cap at 3").
pub const MAX_RECONNECT_ATTEMPTS: u32 = 3;

/// Everything that can go wrong talking to `txtodod`. The daemon being absent or disconnected is
/// a state the event loop renders (a one-line banner), never a panic.
#[derive(Debug)]
pub enum DaemonError {
    /// The endpoint or channel could not be built.
    Connect(tonic::transport::Error),
    /// The daemon answered with a gRPC error.
    Rpc(tonic::Status),
    /// Waiting for the daemon to answer ran past its bound.
    Timeout,
    /// This platform has no working transport yet (Windows: a later milestone, same as desktop).
    UnsupportedPlatform,
}

impl fmt::Display for DaemonError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DaemonError::Connect(e) => write!(f, "connect: {e}"),
            DaemonError::Rpc(e) => write!(f, "daemon rpc: {e}"),
            DaemonError::Timeout => write!(f, "daemon did not become ready in time"),
            DaemonError::UnsupportedPlatform => {
                write!(f, "unix sockets only for now")
            }
        }
    }
}

impl std::error::Error for DaemonError {}

impl From<tonic::Status> for DaemonError {
    fn from(status: tonic::Status) -> Self {
        DaemonError::Rpc(status)
    }
}

/// A client to one workspace's `txtodod`, dialed lazily over its ADR 0010 unix socket. The TUI
/// never opens `todo.txt` itself — every byte comes from [`Daemon::get_file`]/[`Daemon::watch`].
pub struct Daemon {
    inner: pb::txtodo_client::TxtodoClient<tonic::transport::Channel>,
    sock: PathBuf,
}

impl Daemon {
    /// The socket path this client was built for.
    pub fn socket(&self) -> &Path {
        &self.sock
    }

    /// Builds a lazily-dialed channel to `sock`. This never blocks: the first RPC drives the
    /// actual unix-socket dial, bounded by [`CONNECT_TIMEOUT`]. A thin span wrapper around
    /// `connect_inner` (`#[instrument]` on the real body overflows) — root todo.txt `logging-tui`.
    /// Ref: <https://docs.rs/tonic/latest/tonic/transport/struct.Endpoint.html#method.connect_lazy>
    #[cfg(unix)]
    #[tracing::instrument(name = "tui.daemon_connect", skip_all)]
    pub async fn connect(sock: &Path) -> Result<Daemon, DaemonError> {
        Self::connect_inner(sock).await
    }

    #[cfg(unix)]
    async fn connect_inner(sock: &Path) -> Result<Daemon, DaemonError> {
        let dst = format!("unix://{}", sock.display());
        let endpoint = tonic::transport::Endpoint::from_shared(dst)
            .map_err(DaemonError::Connect)?
            .connect_timeout(CONNECT_TIMEOUT);
        let channel = endpoint.connect_lazy();
        Ok(Daemon {
            inner: pb::txtodo_client::TxtodoClient::new(channel),
            sock: sock.to_path_buf(),
        })
    }

    /// Stub for non-unix targets.
    #[cfg(not(unix))]
    pub async fn connect(_sock: &Path) -> Result<Daemon, DaemonError> {
        Err(DaemonError::UnsupportedPlatform)
    }

    /// Probes `Health` up to [`MAX_CONNECT_RETRIES`] times, [`RETRY_BACKOFF`] apart — the "daemon
    /// absent" banner (design §7 edge cases) is what a caller shows when this returns `Err`. A
    /// thin span wrapper around `wait_until_ready_inner` (`#[instrument]` on the real body
    /// overflows) — root todo.txt `logging-tui`.
    #[tracing::instrument(name = "tui.wait_until_ready", skip_all)]
    pub async fn wait_until_ready(&mut self) -> Result<(), DaemonError> {
        self.wait_until_ready_inner().await
    }

    async fn wait_until_ready_inner(&mut self) -> Result<(), DaemonError> {
        let mut last: Option<DaemonError> = None;
        for attempt in 0..MAX_CONNECT_RETRIES {
            let ok = match self.health().await {
                Ok(_health) => true,
                Err(e) => {
                    last = Some(e);
                    false
                }
            };
            log_ready_attempt(attempt, ok);
            if ok {
                return Ok(());
            }
            if attempt + 1 < MAX_CONNECT_RETRIES {
                tokio::time::sleep(RETRY_BACKOFF).await;
            }
        }
        Err(last.unwrap_or(DaemonError::Timeout))
    }

    /// Liveness only; used by [`Daemon::wait_until_ready`].
    async fn health(&mut self) -> Result<pb::HealthResponse, DaemonError> {
        Ok(self
            .inner
            .health(pb::HealthRequest { workspace: None })
            .await?
            .into_inner())
    }

    /// The exact bytes the daemon holds for one workspace-relative document path. The TUI paints
    /// only what this (or `Watch`) returns — it never opens the file itself (design §7).
    pub async fn get_file(&mut self, path: &str) -> Result<pb::FileContents, DaemonError> {
        let req = pb::GetFileRequest {
            path: path.to_owned(),
            workspace: None,
        };
        Ok(self.inner.get_file(req).await?.into_inner())
    }

    /// A change per reconcile or apply for `paths` (every document when empty).
    pub async fn watch(
        &mut self,
        paths: Vec<String>,
    ) -> Result<tonic::Streaming<pb::Change>, DaemonError> {
        let req = pb::WatchRequest {
            paths,
            workspace: None,
        };
        Ok(self.inner.watch(req).await?.into_inner())
    }

    /// Intent-level mutations (`dd`/`Space`/`i`+save); the daemon turns them into ops.
    pub async fn apply(&mut self, req: pb::ApplyRequest) -> Result<pb::ApplyResponse, DaemonError> {
        Ok(self.inner.apply(req).await?.into_inner())
    }

    /// Open `needs_review` flags for `path` (plan M4), for the `r` pane.
    pub async fn list_conflicts(
        &mut self,
        path: &str,
    ) -> Result<pb::ConflictsResponse, DaemonError> {
        let req = pb::ConflictsRequest {
            path: path.to_owned(),
            workspace: None,
        };
        Ok(self.inner.list_conflicts(req).await?.into_inner())
    }

    /// Resolves one `needs_review` flag (`ui/conflicts.rs::resolve_request`'s mine/theirs/merged
    /// pick) — the daemon writes the chosen text back and clears the flag, both or neither.
    pub async fn resolve(
        &mut self,
        req: pb::ResolveRequest,
    ) -> Result<pb::ApplyResponse, DaemonError> {
        Ok(self.inner.resolve_conflict(req).await?.into_inner())
    }
}

/// Split out so the event macro doesn't count against `wait_until_ready`'s own `#[instrument]`
/// budget — the same pattern `crates/txtodo-daemon/src/watcher.rs::log_directory_event` uses.
/// Never logs the socket path or any RPC payload, only the bounded retry counter and outcome.
fn log_ready_attempt(attempt: u32, ok: bool) {
    tracing::debug!(attempt, ok, "ready_attempt");
}

/// Builds the ADR 0010 socket path for a workspace root: `<workspace>/.txtodo/txtodod.sock`.
pub fn socket_path(workspace: &Path) -> PathBuf {
    workspace.join(".txtodo").join("txtodod.sock")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn socket_path_is_the_adr_0010_shape() {
        let ws = Path::new("/tmp/my-workspace");
        assert_eq!(
            socket_path(ws),
            Path::new("/tmp/my-workspace/.txtodo/txtodod.sock")
        );
    }

    #[tokio::test]
    async fn connect_never_blocks_even_with_no_daemon_listening() {
        // `connect_lazy` defers the actual dial to the first RPC; building the client against a
        // socket path that doesn't exist must still succeed immediately (design §7: "connect"
        // failing is what `wait_until_ready` surfaces, not `connect` itself).
        let sock = std::env::temp_dir().join("txtodo-tui-test-no-such-daemon.sock");
        let daemon = Daemon::connect(&sock).await;
        assert!(daemon.is_ok(), "connect_lazy must not dial eagerly");
    }

    #[tokio::test]
    async fn wait_until_ready_times_out_without_a_daemon() {
        let sock = std::env::temp_dir().join("txtodo-tui-test-no-such-daemon-2.sock");
        let mut daemon = Daemon::connect(&sock).await.unwrap();
        let result = daemon.wait_until_ready().await;
        assert!(result.is_err(), "no daemon is listening on this socket");
    }
}
