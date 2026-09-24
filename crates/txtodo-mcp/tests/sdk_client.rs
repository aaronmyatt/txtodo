//! Task mcp-sdk-smoke: rmcp's own stock client against the real `txtodo-mcp` server, over both
//! transports, backed by a real `txtodod`. `tests/smoke.rs` already drives an rmcp client, but
//! in-process over a duplex pipe against a `FakeBackend`; here the client speaks to the shipped
//! binary's stdio and to the real Streamable HTTP router, so a protocol drift between the SDK's
//! client and this server fails a test instead of a user's agent.
//!
//! - Stdio: spawns `txtodo-mcp --dir <workspace> --stdio` and hands its piped stdout/stdin to rmcp.
//! - HTTP: the binary's `--http` binds the fixed `MCP_PORT` (8636), which a test must not take
//!   from a developer's running server, so this serves the same router in-process on a free
//!   loopback port via `transport::serve_http`, over a `GrpcMcpBackend` dialing the real daemon.
//!
//! Each test lists the tools, calls `todo_add` (a write), then `todo_list` (a read) and checks the
//! added line comes back through the daemon.
//!
//! Hermetic: `txtodod --dir <workspace>` keeps its socket, pid lock, registry and logs under
//! `<workspace>/.txtodo/` (`txtodo_workspace_paths::global_socket_path`/`registry_db_path_for`
//! with a legacy dir and no `TXTODO_SOCKET`/`TXTODO_REGISTRY_DB`, which are removed from its env
//! below). `--no-lan --no-relay` keep it off the network. `TXTODO_TEST_KEYSTORE_MEMORY=1` and
//! `TXTODO_NO_SERVICE=1` come from `.cargo/config.toml`'s `[env]` and are set again here so the
//! test is not one config edit away from the macOS keychain or the user's launchd job.
//! `TXTODO_NO_AUTOSTART=1` stops the stdio child from spawning a daemon of its own.
//!
//! `#[ignore]`d like every sibling real-daemon test (see `tests/daemon_autostart.rs`'s module doc):
//! CI runs it in ci.yml's `--run-ignored ignored-only` step. Locally:
//! `cargo test -p txtodo-mcp --test sdk_client -- --ignored`.
#![cfg(unix)] // unix-socket daemon; ci.yml skips the ignored step on Windows (ADR 0010)
#![allow(clippy::expect_used)] // helpers below are not #[test] fns; clippy only exempts those

mod support;

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::{Duration, Instant};

use rmcp::ServiceExt;
use rmcp::model::CallToolRequestParams;
use rmcp::service::{RoleClient, RunningService};
use rmcp::transport::StreamableHttpClientTransport;
use serde_json::json;
use tokio_util::sync::CancellationToken;
use txtodo_mcp::grpc_backend::{GrpcMcpBackend, SOCKET_REL};
use txtodo_mcp::schema::McpServer;
use txtodo_mcp::transport::{MCP_LOOPBACK, MCP_PATH, serve_http};

/// `()` is rmcp's blanket no-op `ClientHandler`, the same client `tests/smoke.rs` uses: these
/// tests never answer a server-initiated request.
/// <https://docs.rs/rmcp/latest/rmcp/handler/client/trait.ClientHandler.html>
type Client = RunningService<RoleClient, ()>;

/// `TXTODO_LOG` (`txtodo_telemetry::LOG_FILTER_ENV`, an `EnvFilter` directive) for both children:
/// their stderr is inherited so a failed start says why, but a child's stderr bypasses the test
/// harness's output capture, so INFO lines would print on every passing run.
/// <https://docs.rs/tracing-subscriber/latest/tracing_subscriber/filter/struct.EnvFilter.html>
const QUIET_LOGS: &str = "warn";

/// A real `txtodod --dir <workspace>`, killed on drop even when an assertion panics first.
struct Daemon {
    child: std::process::Child,
    /// Removed only after `Drop::drop` below has killed the daemon: a type's own `drop` runs
    /// before its fields are dropped. <https://doc.rust-lang.org/reference/destructors.html>
    workspace: tempfile::TempDir,
}

impl Daemon {
    /// The canonical workspace root, as `txtodod`/`txtodo-mcp` canonicalize `--dir` themselves.
    fn root(&self) -> PathBuf {
        self.workspace
            .path()
            .canonicalize()
            .expect("canonical workspace root")
    }

    fn socket(&self) -> PathBuf {
        self.root().join(SOCKET_REL)
    }
}

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Starts `txtodod` on a fresh workspace and waits for its socket. The workspace lives under
/// `/tmp`, not `$TMPDIR` (a long `/var/folders/...` path on macOS): a unix socket path must fit
/// `sun_path`, 104 bytes on macOS and 108 on Linux.
/// <https://man7.org/linux/man-pages/man7/unix.7.html>
/// <https://docs.rs/tempfile/latest/tempfile/struct.Builder.html#method.tempdir_in>
async fn start_daemon() -> Daemon {
    let workspace = tempfile::Builder::new()
        .prefix("mcpsdk")
        .tempdir_in("/tmp")
        .expect("tempdir under /tmp");
    std::fs::write(workspace.path().join("todo.txt"), "buy milk\n").expect("seed todo.txt");
    let child = std::process::Command::new(support::daemon_bin())
        .arg("--dir")
        .arg(workspace.path())
        .args(["--no-lan", "--no-relay"])
        .env_remove("TXTODO_SOCKET")
        .env_remove("TXTODO_REGISTRY_DB")
        .env("TXTODO_TEST_KEYSTORE_MEMORY", "1")
        .env("TXTODO_NO_SERVICE", "1")
        .env("TXTODO_LOG", QUIET_LOGS)
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("spawn txtodod");
    let daemon = Daemon { child, workspace };
    let socket = daemon.socket();
    assert!(
        socket.as_os_str().len() < 100,
        "socket path too long for sun_path: {socket:?}"
    );
    wait_until(|| socket.exists(), &format!("txtodod socket {socket:?}")).await;
    daemon
}

/// Polls `ready` every 50 ms for up to `support::WAIT` (a debug build on a loaded runner is slow).
async fn wait_until(mut ready: impl FnMut() -> bool, what: &str) {
    let start = Instant::now();
    while !ready() {
        assert!(start.elapsed() < support::WAIT, "{what} never appeared");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// `tools/call` with a JSON object of arguments; a protocol error or an `isError` result fails.
/// <https://docs.rs/rmcp/latest/rmcp/service/struct.Peer.html#method.call_tool>
async fn call(client: &Client, tool: &'static str, args: serde_json::Value) -> String {
    let args = args
        .as_object()
        .cloned()
        .expect("arguments are a JSON object");
    let result = client
        .peer()
        .call_tool(CallToolRequestParams::new(tool).with_arguments(args))
        .await
        .unwrap_or_else(|e| panic!("tools/call {tool}: {e}"));
    let text = serde_json::to_string(&result).expect("CallToolResult serializes");
    assert_ne!(
        result.is_error,
        Some(true),
        "{tool} reported an error: {text}"
    );
    text
}

/// The same checks over either transport: the tool list names the read and write tools, a line
/// added with `todo_add` comes back from `todo_list`, and the pre-seeded line is there too (so the
/// read really went to this workspace's daemon, not an empty stand-in).
async fn add_then_list(client: &Client, line: &str) {
    let tools = client
        .peer()
        .list_tools(None)
        .await
        .expect("tools/list succeeds");
    let names: Vec<&str> = tools.tools.iter().map(|t| t.name.as_ref()).collect();
    for wanted in ["todo_list", "todo_add"] {
        assert!(names.contains(&wanted), "{wanted} missing from {names:?}");
    }
    call(client, "todo_add", json!({ "text": line })).await;
    let listed = call(client, "todo_list", json!({})).await;
    assert!(listed.contains(line), "todo_list lacks {line:?}: {listed}");
    assert!(
        listed.contains("buy milk"),
        "todo_list lacks the seeded line: {listed}"
    );
}

#[ignore = "spawns a real txtodod; CI-only, see ci.yml's --ignored step"]
#[tokio::test]
async fn stock_rmcp_client_over_stdio_adds_and_lists_on_a_real_daemon() {
    let daemon = start_daemon().await;
    // `kill_on_drop`: a failed assertion must not leave the server running.
    // <https://docs.rs/tokio/latest/tokio/process/struct.Command.html#method.kill_on_drop>
    let mut server = tokio::process::Command::new(env!("CARGO_BIN_EXE_txtodo-mcp"))
        .arg("--dir")
        .arg(daemon.root())
        .arg("--stdio")
        .env("TXTODO_NO_AUTOSTART", "1")
        .env("TXTODO_NO_SERVICE", "1")
        .env("TXTODO_LOG", QUIET_LOGS)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .kill_on_drop(true)
        .spawn()
        .expect("spawn txtodo-mcp --stdio");
    let stdout = server.stdout.take().expect("piped stdout");
    let stdin = server.stdin.take().expect("piped stdin");

    // An `(AsyncRead, AsyncWrite)` pair is an rmcp transport (the "transport-async-rw" feature,
    // newline-delimited JSON-RPC: the stdio framing the MCP spec defines).
    // <https://docs.rs/rmcp/latest/rmcp/transport/async_rw/index.html>
    // <https://modelcontextprotocol.io/specification/2025-06-18/basic/transports#stdio>
    let client = ().serve((stdout, stdin)).await.expect("stdio client initializes");
    add_then_list(&client, "sdk smoke over stdio +mcp").await;
    client.cancel().await.expect("client cancels cleanly");

    // Cancelling drops the client's end of the child's stdin; the server sees EOF and exits 0.
    let status = tokio::time::timeout(support::WAIT, server.wait())
        .await
        .expect("txtodo-mcp exits after its stdin closes")
        .expect("wait on txtodo-mcp");
    assert!(status.success(), "txtodo-mcp --stdio exited with {status}");
}

/// A loopback port nothing holds right now: bound with port 0, then released.
/// <https://doc.rust-lang.org/std/net/struct.TcpListener.html#method.bind>
fn free_loopback_port() -> u16 {
    std::net::TcpListener::bind((MCP_LOOPBACK, 0))
        .and_then(|l| l.local_addr())
        .expect("free loopback port")
        .port()
}

/// Dials `socket` as `txtodo-mcp` would, then serves the real router on a free loopback port.
/// Returns the endpoint URL, the token that stops the server, and its task.
async fn serve_http_on_real_daemon(
    socket: &Path,
) -> (
    String,
    CancellationToken,
    tokio::task::JoinHandle<Result<(), txtodo_mcp::transport::TransportError>>,
) {
    let backend = GrpcMcpBackend::connect_unix(socket, None)
        .await
        .unwrap_or_else(|e| panic!("connect to txtodod: {e}"));
    let port = free_loopback_port();
    let ct = CancellationToken::new();
    let serving = tokio::spawn(serve_http(
        McpServer::new(Arc::new(backend)),
        port,
        ct.clone(),
    ));
    let addr = std::net::SocketAddr::new(MCP_LOOPBACK, port);
    let start = Instant::now();
    while tokio::net::TcpStream::connect(addr).await.is_err() {
        assert!(
            start.elapsed() < support::WAIT,
            "serve_http never answered on {addr}"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    (format!("http://{addr}{MCP_PATH}"), ct, serving)
}

#[ignore = "spawns a real txtodod; CI-only, see ci.yml's --ignored step"]
#[tokio::test]
async fn stock_rmcp_client_over_streamable_http_adds_and_lists_on_a_real_daemon() {
    let daemon = start_daemon().await;
    let (url, ct, serving) = serve_http_on_real_daemon(&daemon.socket()).await;

    // rmcp's reqwest-backed Streamable HTTP client, stock config. It sends `Host: 127.0.0.1:<port>`
    // and no `Origin`, which `transport::http_router`'s guard serves (see tests/http_guard.rs).
    // <https://docs.rs/rmcp/latest/rmcp/transport/streamable_http_client/type.StreamableHttpClientTransport.html>
    // <https://modelcontextprotocol.io/specification/2025-06-18/basic/transports#streamable-http>
    let transport = StreamableHttpClientTransport::from_uri(url);
    let client = ().serve(transport).await.expect("HTTP client initializes");
    add_then_list(&client, "sdk smoke over streamable http +mcp").await;
    client.cancel().await.expect("client cancels cleanly");

    ct.cancel();
    let stopped = serving.await.expect("serve_http task joins");
    assert!(stopped.is_ok(), "serve_http: {stopped:?}");
}
