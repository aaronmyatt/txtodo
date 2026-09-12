//! The gRPC bridge to `txtodod`: a lazily-dialed, retrying client over the ADR 0010 unix socket,
//! plus the "make sure a daemon exists" bootstrap (design §5/§7, plan M7).
//!
//! The transport is unix-only for now (`#[cfg(unix)]`): Windows named pipes land with the
//! per-user service (`daemon-service-files`, plan M10). On every other target the functions here
//! return [`DaemonError::UnsupportedPlatform`] instead of assuming a `unix:` socket works.

mod spawn;

pub use spawn::ensure_daemon;

use std::fmt;
use std::path::{Path, PathBuf};
use std::time::Duration;
use txtodo_proto::v1 as pb;

/// Per-attempt bound for `wait_until_ready`'s `Health` probe.
const CONNECT_TIMEOUT: Duration = Duration::from_millis(500);
/// How many probe attempts `wait_until_ready` makes before giving up.
const MAX_CONNECT_RETRIES: u32 = 5;
/// Delay between probe attempts.
const RETRY_BACKOFF: Duration = Duration::from_millis(150);

/// Everything that can go wrong talking to `txtodod`. Callers turn this into a [`super::status::DaemonStatus`]
/// change, never a panic (design §5: "daemon-absent/disconnected is a state, never a panic").
#[derive(Debug)]
pub enum DaemonError {
    /// The endpoint or channel could not be built.
    Connect(tonic::transport::Error),
    /// The daemon answered with a gRPC error.
    Rpc(tonic::Status),
    /// `txtodod` could not be spawned.
    Spawn(std::io::Error),
    /// The client-side no-double-spawn lock could not be taken.
    Lock(std::io::Error),
    /// Waiting for the daemon to answer ran past its bound.
    Timeout,
    /// This platform has no working transport yet (Windows: M10).
    UnsupportedPlatform,
}

impl fmt::Display for DaemonError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DaemonError::Connect(e) => write!(f, "connect: {e}"),
            DaemonError::Rpc(e) => write!(f, "daemon rpc: {e}"),
            DaemonError::Spawn(e) => write!(f, "spawn txtodod: {e}"),
            DaemonError::Lock(e) => write!(f, "spawn lock: {e}"),
            DaemonError::Timeout => write!(f, "daemon did not become ready in time"),
            DaemonError::UnsupportedPlatform => {
                write!(f, "Windows transport lands M10; unix sockets only for now")
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

/// A client to one workspace's `txtodod`, dialed lazily over its ADR 0010 unix socket.
pub struct DaemonClient {
    inner: pb::txtodo_client::TxtodoClient<tonic::transport::Channel>,
    sock: PathBuf,
}

impl DaemonClient {
    /// The socket path this client was built for.
    pub fn socket(&self) -> &Path {
        &self.sock
    }

    /// Builds a lazily-dialed channel to `sock`. This never blocks: the first RPC drives the
    /// actual unix-socket dial, bounded by [`CONNECT_TIMEOUT`].
    /// Ref: <https://docs.rs/tonic/latest/tonic/transport/struct.Endpoint.html#method.connect_lazy>
    #[cfg(unix)]
    pub async fn connect(sock: &Path) -> Result<DaemonClient, DaemonError> {
        let dst = format!("unix://{}", sock.display());
        let endpoint = tonic::transport::Endpoint::from_shared(dst)
            .map_err(DaemonError::Connect)?
            .connect_timeout(CONNECT_TIMEOUT);
        let channel = endpoint.connect_lazy();
        Ok(DaemonClient {
            inner: pb::txtodo_client::TxtodoClient::new(channel),
            sock: sock.to_path_buf(),
        })
    }

    /// Stub for non-unix targets; plan M10 adds the Windows named-pipe transport.
    #[cfg(not(unix))]
    pub async fn connect(_sock: &Path) -> Result<DaemonClient, DaemonError> {
        Err(DaemonError::UnsupportedPlatform)
    }

    /// Probes `Health` up to [`MAX_CONNECT_RETRIES`] times, [`RETRY_BACKOFF`] apart, so a
    /// freshly spawned daemon has time to bind the socket before the first real call.
    pub async fn wait_until_ready(&mut self) -> Result<(), DaemonError> {
        let mut last: Option<DaemonError> = None;
        for attempt in 0..MAX_CONNECT_RETRIES {
            match self.health().await {
                Ok(_health) => return Ok(()),
                Err(e) => last = Some(e),
            }
            if attempt + 1 < MAX_CONNECT_RETRIES {
                tokio::time::sleep(RETRY_BACKOFF).await;
            }
        }
        Err(last.unwrap_or(DaemonError::Timeout))
    }

    /// Liveness only; used by [`DaemonClient::wait_until_ready`].
    async fn health(&mut self) -> Result<pb::HealthResponse, DaemonError> {
        Ok(self.inner.health(pb::HealthRequest {}).await?.into_inner())
    }

    /// Every synced document with its current projection hash.
    pub async fn list_files(&mut self) -> Result<pb::ListFilesResponse, DaemonError> {
        let req = pb::ListFilesRequest {};
        Ok(self.inner.list_files(req).await?.into_inner())
    }

    /// The exact bytes the daemon holds for one workspace-relative document path.
    pub async fn get_file(&mut self, path: &str) -> Result<pb::FileContents, DaemonError> {
        let req = pb::GetFileRequest {
            path: path.to_owned(),
        };
        Ok(self.inner.get_file(req).await?.into_inner())
    }

    /// A change per reconcile or apply for `paths` (every document when empty).
    pub async fn watch(
        &mut self,
        paths: Vec<String>,
    ) -> Result<tonic::Streaming<pb::Change>, DaemonError> {
        let req = pb::WatchRequest { paths };
        Ok(self.inner.watch(req).await?.into_inner())
    }

    /// Intent-level mutations; the daemon turns them into ops.
    pub async fn apply(&mut self, req: pb::ApplyRequest) -> Result<pb::ApplyResponse, DaemonError> {
        Ok(self.inner.apply(req).await?.into_inner())
    }

    /// Ops newest first, filtered by path and/or task.
    pub async fn history(
        &mut self,
        req: pb::HistoryRequest,
    ) -> Result<pb::HistoryResponse, DaemonError> {
        Ok(self.inner.history(req).await?.into_inner())
    }

    /// Resolves one `needs_review` flag; maps to the daemon's `ResolveConflict` RPC.
    pub async fn resolve(
        &mut self,
        req: pb::ResolveRequest,
    ) -> Result<pb::ApplyResponse, DaemonError> {
        Ok(self.inner.resolve_conflict(req).await?.into_inner())
    }
}
