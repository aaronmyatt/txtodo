//! Daemon mode (plan M3): when `<dir>/.txtodo/txtodod.sock` exists the CLI talks to txtodod over
//! gRPC instead of touching files. The socket is the slice boundary — this crate never imports the
//! daemon. A socket that exists but refuses the connection is an error (a stale socket after a
//! crash is worth surfacing), not a silent fallback; `--no-daemon` forces direct-file mode.
//! tonic over UDS: https://github.com/hyperium/tonic/tree/master/examples/src/uds

use std::fmt;
use std::path::{Path, PathBuf};
use tonic::transport::Channel;
use txtodo_proto::v1::txtodo_client::TxtodoClient;
use txtodo_proto::v1::{self as pb};
// Unix-domain sockets are the daemon's only transport (ADR 0010), and tokio gates `UnixStream` to
// unix targets; on Windows the connector is compiled out and `connect` refuses instead.
#[cfg(unix)]
use hyper_util::rt::TokioIo;
#[cfg(unix)]
use tokio::net::UnixStream;
#[cfg(unix)]
use tonic::transport::{Endpoint, Uri};
#[cfg(unix)]
use tower::service_fn;

/// Where the daemon listens, relative to the todo dir (ADR 0010).
pub const SOCKET_REL: &str = ".txtodo/txtodod.sock";

/// How this invocation reaches the files.
pub enum Mode {
    /// Through txtodod (boxed: the runtime and channel are large next to `Direct`).
    Daemon(Box<Daemon>),
    /// Straight to disk (M2 behaviour).
    Direct,
}

/// A connected client with its own current-thread runtime; direct mode never builds one.
pub struct Daemon {
    rt: tokio::runtime::Runtime,
    client: TxtodoClient<Channel>,
}

/// Why daemon mode failed.
#[derive(Debug)]
pub enum ClientError {
    /// The socket exists but nobody answers; `txtodo doctor` explains, `txtodo daemon start` fixes.
    SocketRefused {
        /// The socket path.
        socket: PathBuf,
        /// The transport error text.
        detail: String,
    },
    /// An RPC failed.
    Rpc(tonic::Status),
    /// The runtime could not start.
    Runtime(std::io::Error),
}

impl fmt::Display for ClientError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ClientError::SocketRefused { socket, detail } => write!(
                f,
                "daemon socket {} exists but refused the connection ({detail}); run `txtodo doctor`, or `txtodo --no-daemon`",
                socket.display()
            ),
            ClientError::Rpc(s) => write!(f, "daemon: {} ({:?})", s.message(), s.code()),
            ClientError::Runtime(e) => write!(f, "cannot start the async runtime: {e}"),
        }
    }
}

impl std::error::Error for ClientError {}

/// Picks the mode for this invocation.
pub fn select(dir: &Path, no_daemon: bool) -> Result<Mode, ClientError> {
    let socket = dir.join(SOCKET_REL);
    if no_daemon || !socket.exists() {
        return Ok(Mode::Direct);
    }
    Daemon::connect(socket).map(|d| Mode::Daemon(Box::new(d)))
}

impl Daemon {
    #[cfg(unix)]
    fn connect(socket: PathBuf) -> Result<Daemon, ClientError> {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(ClientError::Runtime)?;
        let dial = socket.clone();
        let channel = rt.block_on(async {
            // tonic needs a URI; the connector ignores it and dials the socket.
            Endpoint::try_from("http://[::]:50051")
                .map_err(|e| e.to_string())?
                .connect_with_connector(service_fn(move |_: Uri| {
                    let dial = dial.clone();
                    async move { UnixStream::connect(dial).await.map(TokioIo::new) }
                }))
                .await
                .map_err(|e| e.to_string())
        });
        let channel = channel.map_err(|detail| ClientError::SocketRefused {
            socket: socket.clone(),
            detail,
        })?;
        debug_assert!(socket.exists(), "connected through an existing socket");
        Ok(Daemon {
            rt,
            client: TxtodoClient::new(channel),
        })
    }

    /// Windows has no unix-domain sockets, so only direct-file mode exists there.
    #[cfg(not(unix))]
    fn connect(socket: PathBuf) -> Result<Daemon, ClientError> {
        Err(ClientError::SocketRefused {
            socket,
            detail: "unix domain sockets are unavailable on this platform".to_owned(),
        })
    }

    /// Current bytes of a document (workspace-relative path).
    pub fn get(&mut self, path: &str) -> Result<Vec<u8>, ClientError> {
        let req = pb::GetFileRequest {
            path: path.to_owned(),
        };
        let rep = self
            .rt
            .block_on(self.client.get_file(req))
            .map_err(ClientError::Rpc)?;
        Ok(rep.into_inner().bytes)
    }

    /// Every synced document.
    pub fn list_files(&mut self) -> Result<Vec<pb::FileInfo>, ClientError> {
        let rep = self
            .rt
            .block_on(self.client.list_files(pb::ListFilesRequest {}))
            .map_err(ClientError::Rpc)?;
        Ok(rep.into_inner().files)
    }

    /// Intent-level mutations against one document.
    pub fn apply(
        &mut self,
        path: &str,
        mutations: Vec<pb::Mutation>,
    ) -> Result<pb::ApplyResponse, ClientError> {
        debug_assert!(!mutations.is_empty(), "callers skip empty batches");
        let req = pb::ApplyRequest {
            path: path.to_owned(),
            mutations,
            agent: None,
        };
        let rep = self
            .rt
            .block_on(self.client.apply(req))
            .map_err(ClientError::Rpc)?;
        Ok(rep.into_inner())
    }

    /// Ops newest first.
    pub fn history(&mut self, req: pb::HistoryRequest) -> Result<Vec<pb::OpSummary>, ClientError> {
        let rep = self
            .rt
            .block_on(self.client.history(req))
            .map_err(ClientError::Rpc)?;
        Ok(rep.into_inner().ops)
    }

    /// Inverts the newest `steps` ops.
    pub fn undo(&mut self, path: &str, steps: u32) -> Result<pb::ApplyResponse, ClientError> {
        let req = pb::UndoRequest {
            path: path.to_owned(),
            steps,
        };
        let rep = self
            .rt
            .block_on(self.client.undo(req))
            .map_err(ClientError::Rpc)?;
        Ok(rep.into_inner())
    }

    /// The document at a wall time (inclusive).
    pub fn checkout(&mut self, path: &str, at_wall_ms: u64) -> Result<Vec<u8>, ClientError> {
        let req = pb::CheckoutRequest {
            path: path.to_owned(),
            at_wall_ms,
        };
        let rep = self
            .rt
            .block_on(self.client.checkout(req))
            .map_err(ClientError::Rpc)?;
        Ok(rep.into_inner().bytes)
    }

    /// Liveness for `txtodo doctor`.
    pub fn health(&mut self) -> Result<pb::HealthResponse, ClientError> {
        let rep = self
            .rt
            .block_on(self.client.health(pb::HealthRequest {}))
            .map_err(ClientError::Rpc)?;
        Ok(rep.into_inner())
    }
}
