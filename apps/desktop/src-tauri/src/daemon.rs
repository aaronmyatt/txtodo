//! The gRPC bridge to `txtodod`: a lazily-dialed, retrying client over the ADR 0010 unix socket,
//! plus the "make sure a daemon exists" bootstrap (design §5/§7, plan M7).
//!
//! The transport is unix-only for now (`#[cfg(unix)]`): Windows named pipes land with the
//! per-user service (`daemon-service-files`, plan M10). On every other target the functions here
//! return [`DaemonError::UnsupportedPlatform`] instead of assuming a `unix:` socket works.

mod spawn;
mod workspace;

pub use spawn::ensure_daemon;

use std::fmt;
use std::path::{Path, PathBuf};
use std::time::Duration;
use txtodo_proto::v1 as pb;

/// Per-attempt bound for `wait_until_ready`'s `Health` probe. Gated with the same `#[cfg(unix)]`
/// as its one use site (`connect`'s unix-socket dial): on Windows that arm is
/// `UnsupportedPlatform` and an ungated const is dead code, which CI's `-D warnings` rejects.
/// Ref: https://doc.rust-lang.org/reference/conditional-compilation.html#the-cfg-attribute
#[cfg(unix)]
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

/// A client to `txtodod` over its ADR 0010 unix socket, dialed lazily — against the true global
/// daemon as of ADR 0025/M11 (task `desktop-workspace-switcher`), so one client now serves every
/// registered workspace by varying [`DaemonClient::selector`], not a fixed one dialed per socket.
pub struct DaemonClient {
    inner: pb::txtodo_client::TxtodoClient<tonic::transport::Channel>,
    sock: PathBuf,
    /// Attached to every request below. `switch_workspace` is the only way to change it — every
    /// in-flight and future RPC on this client targets whichever workspace it last named.
    selector: Option<pb::WorkspaceSelector>,
}

impl DaemonClient {
    /// The socket path this client was built for.
    pub fn socket(&self) -> &Path {
        &self.sock
    }

    /// Points every future RPC at `root` instead of whatever workspace this client targeted
    /// before — no reconnect, since the global daemon is one process serving every registered
    /// workspace already. `root` auto-registers via the same `Path` selector bridge
    /// `WorkspaceCatalog::resolve` already proves at the daemon level.
    pub fn switch_workspace(&mut self, root: &Path) {
        self.selector = Some(pb::WorkspaceSelector {
            selector: Some(pb::workspace_selector::Selector::Path(
                root.display().to_string(),
            )),
        });
    }

    /// Builds a lazily-dialed channel to `sock`, targeting `selector` (`None` means "the sole
    /// open workspace", the same bridge every other client of the global daemon relies on until
    /// `switch_workspace` names one explicitly). This never blocks: the first RPC drives the
    /// actual unix-socket dial, bounded by [`CONNECT_TIMEOUT`].
    /// Ref: <https://docs.rs/tonic/latest/tonic/transport/struct.Endpoint.html#method.connect_lazy>
    #[cfg(unix)]
    pub async fn connect(
        sock: &Path,
        selector: Option<pb::WorkspaceSelector>,
    ) -> Result<DaemonClient, DaemonError> {
        let dst = format!("unix://{}", sock.display());
        let endpoint = tonic::transport::Endpoint::from_shared(dst)
            .map_err(DaemonError::Connect)?
            .connect_timeout(CONNECT_TIMEOUT);
        let channel = endpoint.connect_lazy();
        Ok(DaemonClient {
            inner: pb::txtodo_client::TxtodoClient::new(channel),
            sock: sock.to_path_buf(),
            selector,
        })
    }

    /// Stub for non-unix targets; plan M10 adds the Windows named-pipe transport.
    #[cfg(not(unix))]
    pub async fn connect(
        _sock: &Path,
        _selector: Option<pb::WorkspaceSelector>,
    ) -> Result<DaemonClient, DaemonError> {
        Err(DaemonError::UnsupportedPlatform)
    }

    /// Probes the daemon up to [`MAX_CONNECT_RETRIES`] times, [`RETRY_BACKOFF`] apart, so a
    /// freshly spawned daemon has time to bind the socket before the first real call. With a
    /// workspace selected that is a `Health` call; with none, `Health` would be refused (an
    /// unselected call needs exactly one open workspace), so the registry-level `workspace_list`
    /// — never selector-scoped — is the probe instead.
    pub async fn wait_until_ready(&mut self) -> Result<(), DaemonError> {
        let mut last: Option<DaemonError> = None;
        for attempt in 0..MAX_CONNECT_RETRIES {
            let probe = if self.selector.is_some() {
                self.health().await.map(drop)
            } else {
                self.workspace_list().await.map(drop)
            };
            match probe {
                Ok(()) => return Ok(()),
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
        let workspace = self.selector.clone();
        Ok(self
            .inner
            .health(pb::HealthRequest { workspace })
            .await?
            .into_inner())
    }

    /// Every synced document with its current projection hash.
    pub async fn list_files(&mut self) -> Result<pb::ListFilesResponse, DaemonError> {
        let req = pb::ListFilesRequest {
            workspace: self.selector.clone(),
        };
        Ok(self.inner.list_files(req).await?.into_inner())
    }

    /// The exact bytes the daemon holds for one workspace-relative document path.
    pub async fn get_file(&mut self, path: &str) -> Result<pb::FileContents, DaemonError> {
        let req = pb::GetFileRequest {
            path: path.to_owned(),
            workspace: self.selector.clone(),
        };
        Ok(self.inner.get_file(req).await?.into_inner())
    }

    /// `get_file`, but against `selector` regardless of what this client's own `selector` field
    /// currently targets — never mutates it. The universal view's only caller: aggregating every
    /// registered workspace's `todo.txt` must not disturb whichever workspace the rest of the app
    /// (and this same connection) already has open.
    pub async fn get_file_for(
        &mut self,
        selector: pb::WorkspaceSelector,
        path: &str,
    ) -> Result<pb::FileContents, DaemonError> {
        let req = pb::GetFileRequest {
            path: path.to_owned(),
            workspace: Some(selector),
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
            workspace: self.selector.clone(),
        };
        Ok(self.inner.watch(req).await?.into_inner())
    }

    /// Intent-level mutations; the daemon turns them into ops.
    pub async fn apply(
        &mut self,
        mut req: pb::ApplyRequest,
    ) -> Result<pb::ApplyResponse, DaemonError> {
        req.workspace = self.selector.clone();
        Ok(self.inner.apply(req).await?.into_inner())
    }

    /// Ops newest first, filtered by path and/or task.
    pub async fn history(
        &mut self,
        mut req: pb::HistoryRequest,
    ) -> Result<pb::HistoryResponse, DaemonError> {
        req.workspace = self.selector.clone();
        Ok(self.inner.history(req).await?.into_inner())
    }

    /// Resolves one `needs_review` flag; maps to the daemon's `ResolveConflict` RPC.
    pub async fn resolve(
        &mut self,
        mut req: pb::ResolveRequest,
    ) -> Result<pb::ApplyResponse, DaemonError> {
        req.workspace = self.selector.clone();
        Ok(self.inner.resolve_conflict(req).await?.into_inner())
    }

    /// Open `needs_review` flags for `path` (plan M4): two devices rewrote the same word.
    pub async fn list_conflicts(
        &mut self,
        path: &str,
    ) -> Result<pb::ConflictsResponse, DaemonError> {
        let req = pb::ConflictsRequest {
            path: path.to_owned(),
            workspace: self.selector.clone(),
        };
        Ok(self.inner.list_conflicts(req).await?.into_inner())
    }

    /// `notes.md` for one task's `ref:` directory (plan M5); the daemon resolves the task to its
    /// directory, this client never touches the filesystem itself.
    pub async fn get_notes(&mut self, task: pb::TaskRef) -> Result<pb::NotesDoc, DaemonError> {
        let req = pb::GetNotesRequest {
            task: Some(task),
            workspace: self.selector.clone(),
        };
        Ok(self.inner.get_notes(req).await?.into_inner())
    }

    /// One whole-document edit to `notes.md`; the daemon derives the Loro text ops and lazily
    /// creates the `ref:` directory on the first edit (plan §3.2.4).
    pub async fn edit_notes(
        &mut self,
        mut req: pb::NotesEditRequest,
    ) -> Result<pb::ApplyResponse, DaemonError> {
        req.workspace = self.selector.clone();
        Ok(self.inner.edit_notes(req).await?.into_inner())
    }

    /// Starts a pairing handshake on this device and returns the QR payload (plan M4, design §4).
    pub async fn pair_offer(&mut self) -> Result<pb::PairOfferResponse, DaemonError> {
        let workspace = self.selector.clone();
        Ok(self
            .inner
            .pair_offer(pb::PairOfferRequest { workspace })
            .await?
            .into_inner())
    }

    /// Accepts a peer's scanned `PairOffer` (`code`) and begins the X25519 handshake; returns the
    /// 6-word SAS.
    pub async fn pair_accept(&mut self, code: String) -> Result<pb::PairResult, DaemonError> {
        let req = pb::PairAcceptRequest {
            code,
            workspace: self.selector.clone(),
        };
        Ok(self.inner.pair_accept(req).await?.into_inner())
    }

    /// Confirms the SAS shown to the human on this device; the group key lands only once both
    /// sides have confirmed.
    pub async fn pair_confirm_sas(&mut self) -> Result<pb::PairResult, DaemonError> {
        let workspace = self.selector.clone();
        Ok(self
            .inner
            .pair_confirm_sas(pb::PairConfirmRequest { workspace })
            .await?
            .into_inner())
    }

    /// Mints a new capability token from the design §6.2 scope/caveat grammar.
    pub async fn token_create(
        &mut self,
        mut req: pb::TokenCreateRequest,
    ) -> Result<pb::Token, DaemonError> {
        req.workspace = self.selector.clone();
        Ok(self.inner.token_create(req).await?.into_inner())
    }

    /// Tokens for this workspace, scopes included, secrets never returned.
    pub async fn token_list(&mut self) -> Result<pb::TokenListResponse, DaemonError> {
        let workspace = self.selector.clone();
        Ok(self
            .inner
            .token_list(pb::TokenListRequest { workspace })
            .await?
            .into_inner())
    }

    /// Revokes a token; the daemon refuses it on its next use.
    pub async fn token_revoke(
        &mut self,
        id: String,
    ) -> Result<pb::TokenRevokeResponse, DaemonError> {
        let req = pb::TokenRevokeRequest {
            id,
            workspace: self.selector.clone(),
        };
        Ok(self.inner.token_revoke(req).await?.into_inner())
    }

    /// Newest ops across every tracked file, at most 200, newest first: one bounded read, not a
    /// live tail (unlike [`DaemonClient::watch`]) — so this drains the whole stream before
    /// returning instead of forwarding it as an event stream.
    pub async fn op_log(&mut self) -> Result<Vec<pb::OpLogEntry>, DaemonError> {
        let workspace = self.selector.clone();
        self.op_log_with(workspace).await
    }

    /// As [`Self::op_log`], but targeting `workspace` explicitly rather than this client's own
    /// ambient `selector` — task `desktop-activity-cross-workspace`'s fan-out needs to read one
    /// workspace's op log while switched to (or connected with no selector on) another.
    pub async fn op_log_for(
        &mut self,
        workspace: pb::WorkspaceSelector,
    ) -> Result<Vec<pb::OpLogEntry>, DaemonError> {
        self.op_log_with(Some(workspace)).await
    }

    async fn op_log_with(
        &mut self,
        workspace: Option<pb::WorkspaceSelector>,
    ) -> Result<Vec<pb::OpLogEntry>, DaemonError> {
        let mut stream = self
            .inner
            .op_log_stream(pb::OpLogRequest { workspace })
            .await?
            .into_inner();
        let mut entries = Vec::new();
        while let Some(entry) = stream.message().await? {
            entries.push(entry);
        }
        Ok(entries)
    }
}
