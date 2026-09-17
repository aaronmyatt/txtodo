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
use txtodo_mcp::global_socket;
use txtodo_mcp::grpc_backend::{GrpcMcpBackend, SOCKET_REL};
use txtodo_mcp::schema::McpServer;
use txtodo_mcp::transport::{self, MCP_LAN, MCP_LOOPBACK, MCP_PORT};

/// Which transport to serve, and how.
enum Mode {
    /// `--stdio`.
    Stdio,
    /// `--http [--lan]`.
    Http {
        /// Bind every interface and advertise `_txtodo-mcp._tcp`, instead of loopback-only.
        lan: bool,
    },
}

/// Which daemon socket to dial (mcp-multi-workspace-gateway): the pre-existing per-workspace
/// bridge (`--dir <workspace>`, unchanged — reaches only whatever workspace(s) that directory's
/// own daemon has opened), or the device-global socket (`--global`, new — reaches every workspace
/// the device's one `txtodod` already has open, the mode a `workspace` tool/resource arg is
/// actually useful against). Mutually exclusive; exactly one is required.
enum Target {
    /// `--dir <workspace>`.
    Dir(PathBuf),
    /// `--global`.
    Global,
}

/// `txtodo-mcp (--dir <workspace> | --global) --stdio|--http [--lan] [--token <id>]`.
struct Args {
    target: Target,
    mode: Mode,
    token: Option<String>,
}

fn parse_args() -> Result<Args, String> {
    let mut args = std::env::args_os().skip(1);
    let (mut dir, mut global, mut mode, mut token, mut lan) = (None, false, None, None, false);
    while let Some(a) = args.next() {
        match a.to_str() {
            Some("--dir") => dir = args.next().map(PathBuf::from),
            Some("--global") => global = true,
            Some("--stdio") => mode = Some(Mode::Stdio),
            Some("--http") => mode = Some(Mode::Http { lan: false }),
            Some("--lan") => lan = true,
            Some("--token") => token = args.next().and_then(|v| v.into_string().ok()),
            _ => return Err(format!("unknown argument {a:?}")),
        }
    }
    let target = match (dir, global) {
        (Some(_), true) => return Err("--dir and --global are mutually exclusive".to_owned()),
        (Some(dir), false) => Target::Dir(dir.canonicalize().map_err(|e| e.to_string())?),
        (None, true) => Target::Global,
        (None, false) => return Err(usage()),
    };
    let mode = match (mode.ok_or_else(usage)?, lan, &token) {
        (Mode::Stdio, true, _) => {
            return Err("--lan --stdio is a usage error: stdio has no network".to_owned());
        }
        (Mode::Http { .. }, true, None) => {
            return Err("--lan without --token would bind 0.0.0.0 with no bearer auth configured; refusing to start".to_owned());
        }
        (Mode::Http { .. }, lan, _) => Mode::Http { lan },
        (stdio, false, _) => stdio,
    };
    Ok(Args {
        target,
        mode,
        token,
    })
}

fn usage() -> String {
    "usage: txtodo-mcp (--dir <workspace> | --global) --stdio|--http [--lan] [--token <id>]"
        .to_owned()
}

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
        Target::Global => txtodo_daemon_launch::LaunchConfig::new(socket),
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
        Target::Global => (global_socket::path(), global_socket::log_dir()),
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
    let server = McpServer::new(Arc::new(backend));
    match args.mode {
        Mode::Stdio => transport::serve_stdio(server).await?,
        Mode::Http { lan } => serve_http(server, lan, args.token.as_deref()).await?,
    }
    Ok(())
}

async fn serve_http(
    server: McpServer,
    lan: bool,
    token: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    let addr = std::net::SocketAddr::new(if lan { MCP_LAN } else { MCP_LOOPBACK }, MCP_PORT);
    let ct = CancellationToken::new();
    let _daemon = lan
        .then(|| transport::advertise_lan("txtodo-mcp.local.", MCP_PORT))
        .transpose()?;
    debug_assert!(
        !lan || token.is_some(),
        "--lan without --token was refused earlier"
    );
    let shutdown = shutdown_signal(ct.clone());
    tokio::select! {
        result = transport::serve_http(server, addr, ct) => result?,
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
