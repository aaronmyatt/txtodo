//! `txtodo-mcp`: the MCP server binary (mcp-transports notes.md). A gRPC client of `txtodod`,
//! wrapped in the stdio or Streamable HTTP transport. `txtodo mcp --stdio|--http` (the CLI's `mcp`
//! subcommand, `crates/txtodo-cli/src/commands/mcp.rs`) execs this binary rather than linking this
//! crate — `budgets.json`'s `allowedDeps` lets `txtodo-daemon` depend on `txtodo-mcp`, not
//! `txtodo-cli`, so the CLI can only launch this as a sibling process. See this crate's As-built
//! notes for the full reasoning.
#![forbid(unsafe_code)]
#![allow(clippy::print_stderr)] // this binary's only human output path (matches txtodod's main.rs)

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;

use tokio_util::sync::CancellationToken;
use txtodo_mcp::backend::McpBackend;
use txtodo_mcp::global_socket;
use txtodo_mcp::grpc_backend::{GrpcMcpBackend, SOCKET_REL};
use txtodo_mcp::schema::McpServer;
use txtodo_mcp::transport::{self, MCP_PORT};

/// Which transport to serve, and how.
enum Mode {
    /// `--stdio`.
    Stdio,
    /// `--http`: Streamable HTTP on loopback. Nothing widens it (task mcp-local-only).
    Http,
}

/// Which daemon socket to dial (mcp-multi-workspace-gateway): the pre-existing per-workspace
/// bridge (`--dir <workspace>`, unchanged — reaches only whatever workspace(s) that directory's
/// own daemon has opened), or the device-global socket (`--global`, new — reaches every workspace
/// the device's one `txtodod` already has open, the mode a `workspace` tool/resource arg is
/// actually useful against). `--dir` and `--global` are mutually exclusive; neither is `Auto`.
enum Target {
    /// `--dir <workspace>`.
    Dir(PathBuf),
    /// `--global`.
    Global,
    /// Neither flag (task default-workspace): the global daemon, aimed at the folder this server
    /// was started in when that is a workspace, else at the user's default workspace.
    Auto,
}

/// `txtodo-mcp [--dir <workspace> | --global] --stdio|--http [--token <id>]`; with neither flag it
/// serves the current folder if that is a workspace, else the default one.
struct Args {
    target: Target,
    mode: Mode,
    token: Option<String>,
}

fn parse_args() -> Result<Args, String> {
    let mut args = std::env::args_os().skip(1);
    let (mut dir, mut global, mut mode, mut token) = (None, false, None, None);
    while let Some(a) = args.next() {
        match a.to_str() {
            Some("--dir") => dir = args.next().map(PathBuf::from),
            Some("--global") => global = true,
            Some("--stdio") => mode = Some(Mode::Stdio),
            Some("--http") => mode = Some(Mode::Http),
            Some("--lan") => return Err(LAN_REMOVED.to_owned()),
            Some("--token") => token = args.next().and_then(|v| v.into_string().ok()),
            _ => return Err(format!("unknown argument {a:?}")),
        }
    }
    let target = match (dir, global) {
        (Some(_), true) => return Err("--dir and --global are mutually exclusive".to_owned()),
        (Some(dir), false) => Target::Dir(dir.canonicalize().map_err(|e| e.to_string())?),
        (None, true) => Target::Global,
        // Neither flag (task default-workspace): the global daemon, and the folder this server was
        // started in when it is a workspace, else the user's default workspace.
        (None, false) => Target::Auto,
    };
    let mode = mode.ok_or_else(usage)?;
    Ok(Args {
        target,
        mode,
        token,
    })
}

fn usage() -> String {
    "usage: txtodo-mcp [--dir <workspace> | --global] --stdio|--http [--token <id>]".to_owned()
}

/// What `--lan` answers now. It used to bind every interface and advertise over mDNS.
const LAN_REMOVED: &str = "--lan was removed: the MCP server is reachable from this device only \
     (127.0.0.1); see tasks/mcp-local-only";

/// Best-effort daemon autostart before dialing `socket` (task `daemon-always-available`, item 5):
/// builds whichever `LaunchConfig` shape matches this run's `Target` (a legacy `--dir <workspace>`
/// bridge daemon, or the ADR 0025 global daemon) and calls `ensure_daemon`. The `Result` is
/// deliberately discarded: the `GrpcMcpBackend::connect_unix` call right after this is the real,
/// honest failure path, and its `ConnectError` ("is txtodod running?") stays the accurate message
/// to show when even this best-effort spawn couldn't produce a reachable daemon (e.g. no `txtodod`
/// anywhere on `$PATH` in this environment). Split out of `run` to keep it under this crate's
/// function-length/complexity budgets (`.claude/budgets.json`: 60 lines, cognitive complexity 10).
async fn ensure_daemon_for_target(target: &Target, socket: &Path) {
    if txtodo_daemon_launch::autostart_disabled() {
        return;
    }
    let cfg = match target {
        Target::Dir(dir) => txtodo_daemon_launch::LaunchConfig::new(socket).with_dir(dir),
        // `with_upgrade_to` (task daemon-auto-upgrade): a live global daemon older than this
        // build is restarted with the newer `txtodod` first. A `--dir` bridge daemon never is.
        Target::Global | Target::Auto => txtodo_daemon_launch::LaunchConfig::new(socket)
            .with_upgrade_to(env!("CARGO_PKG_VERSION")),
    };
    let _ = txtodo_daemon_launch::ensure_daemon(&cfg).await;
}

fn main() -> ExitCode {
    let args = match parse_args() {
        Ok(a) => a,
        Err(msg) => {
            eprintln!("{msg}");
            return ExitCode::from(2);
        }
    };
    let rt = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("cannot start the async runtime: {e}");
            return ExitCode::FAILURE;
        }
    };
    match rt.block_on(run(args)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("txtodo-mcp: {e}");
            ExitCode::FAILURE
        }
    }
}

async fn run(args: Args) -> Result<(), Box<dyn std::error::Error>> {
    // `--dir`: the pre-existing per-workspace bridge socket/logs, unchanged. `--global`: the
    // device-global socket (`global_socket::path`) and its own log directory
    // (`global_socket::log_dir`) — a `--global`-started server has no single workspace directory
    // to put `.txtodo/logs/` under.
    let (socket, log_dir) = match &args.target {
        Target::Dir(dir) => (dir.join(SOCKET_REL), dir.join(".txtodo/logs")),
        Target::Global | Target::Auto => (global_socket::path(), global_socket::log_dir()),
    };
    // JSON rolling-file + pretty-stderr layer (never stdout — `transport::serve_stdio` owns
    // stdin/stdout for the MCP protocol itself, see `transport.rs`'s `rmcp::transport::io::stdio`
    // call and this crate's As-built notes). `--dir` logs share the daemon's `.txtodo/logs/`
    // directory (own `txtodo-mcp.log.YYYY-MM-DD` file family via the `service` name) so
    // `txtodo doctor` and a human tailing the directory see every process's lines in one place;
    // `--global` mirrors that placement beside the global socket instead. `_log_guard` must
    // outlive every `tracing::` call below — held for `run`'s whole body, dropped only on return.
    let _log_guard = txtodo_telemetry::init("txtodo-mcp", &log_dir)?;
    let agent = args.token.clone().map(|t| (t, "mcp".to_owned()));
    ensure_daemon_for_target(&args.target, &socket).await;
    let backend = GrpcMcpBackend::connect_unix(&socket, agent).await?;
    if matches!(args.target, Target::Auto) {
        aim_at_the_current_workspace(&backend).await;
    }
    let server = McpServer::new(Arc::new(backend));
    match args.mode {
        Mode::Stdio => transport::serve_stdio(server).await?,
        Mode::Http => serve_http(server).await?,
    }
    Ok(())
}

/// With neither `--dir` nor `--global`: calls that name no `workspace` mean the folder this server
/// was started in when it is a workspace *the daemon already knows* (registered by the CLI or the
/// desktop), else the default workspace (task default-workspace) — and stderr says which. Never
/// registers the folder itself (task mcp-cwd-autoregister): naming an unknown path would add it
/// to the registry and announce its name to every paired peer, so a stray `todo.txt` folder an
/// agent happens to start in stays private. stdout is the MCP protocol, so it is never used here.
#[allow(clippy::print_stderr)]
async fn aim_at_the_current_workspace(backend: &GrpcMcpBackend) {
    let env = txtodo_workspace_paths::RegistryEnv::from_process().unwrap_or_default();
    let cwd = std::env::current_dir().unwrap_or_default();
    match txtodo_workspace_paths::choose_workspace(&env, &cwd) {
        txtodo_workspace_paths::WorkspaceChoice::Here(dir) => {
            let known = backend.list_workspaces().await.unwrap_or_default();
            if is_registered(&known, &dir) {
                txtodo_mcp::set_default_workspace(dir.display().to_string());
            } else {
                eprintln!(
                    "txtodo-mcp: {} is a workspace the daemon does not know; using the default \
                     workspace instead. To serve it, register it first: txtodo workspace add {}",
                    dir.display(),
                    dir.display()
                );
            }
        }
        txtodo_workspace_paths::WorkspaceChoice::Default(dir) => {
            eprintln!(
                "txtodo-mcp: no workspace here, using the default workspace ({})",
                dir.display()
            );
        }
    }
}

/// Whether `dir` is one of `known`'s roots. The registry stores canonicalized roots, so `dir` is
/// compared both as given and canonicalized (macOS's `/tmp` → `/private/tmp`).
fn is_registered(known: &[txtodo_mcp::backend::WorkspaceInfo], dir: &Path) -> bool {
    let canon = dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf());
    known
        .iter()
        .map(|w| Path::new(&w.root))
        .any(|root| root == dir || root == canon)
}

async fn serve_http(server: McpServer) -> Result<(), Box<dyn std::error::Error>> {
    let ct = CancellationToken::new();
    let shutdown = shutdown_signal(ct.clone());
    tokio::select! {
        result = transport::serve_http(server, MCP_PORT, ct) => result?,
        () = shutdown => {}
    }
    Ok(())
}

/// SIGTERM/SIGINT, matching `txtodod`'s own shutdown listener.
async fn shutdown_signal(ct: CancellationToken) {
    let ctrl_c = tokio::signal::ctrl_c();
    #[cfg(unix)]
    {
        let mut term =
            match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
                Ok(s) => s,
                Err(_) => {
                    let _ = ctrl_c.await;
                    ct.cancel();
                    return;
                }
            };
        tokio::select! { _ = ctrl_c => {}, _ = term.recv() => {} }
    }
    #[cfg(not(unix))]
    {
        let _ = ctrl_c.await;
    }
    ct.cancel();
}
