//! `txtodo-mcp`: the MCP server binary (mcp-transports notes.md). A gRPC client of `txtodod`,
//! wrapped in the stdio or Streamable HTTP transport. `txtodo mcp --stdio|--http` (the CLI's `mcp`
//! subcommand, `crates/txtodo-cli/src/commands/mcp.rs`) execs this binary rather than linking this
//! crate — `budgets.json`'s `allowedDeps` lets `txtodo-daemon` depend on `txtodo-mcp`, not
//! `txtodo-cli`, so the CLI can only launch this as a sibling process. See this crate's As-built
//! notes for the full reasoning.
#![forbid(unsafe_code)]
#![allow(clippy::print_stderr)] // this binary's only human output path (matches txtodod's main.rs)

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

use tokio_util::sync::CancellationToken;
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

/// `txtodo-mcp --dir <workspace> --stdio|--http [--lan] [--token <id>]`.
struct Args {
    dir: PathBuf,
    mode: Mode,
    token: Option<String>,
}

fn parse_args() -> Result<Args, String> {
    let mut args = std::env::args_os().skip(1);
    let (mut dir, mut mode, mut token, mut lan) = (None, None, None, false);
    while let Some(a) = args.next() {
        match a.to_str() {
            Some("--dir") => dir = args.next().map(PathBuf::from),
            Some("--stdio") => mode = Some(Mode::Stdio),
            Some("--http") => mode = Some(Mode::Http { lan: false }),
            Some("--lan") => lan = true,
            Some("--token") => token = args.next().and_then(|v| v.into_string().ok()),
            _ => return Err(format!("unknown argument {a:?}")),
        }
    }
    let dir = dir
        .ok_or_else(usage)?
        .canonicalize()
        .map_err(|e| e.to_string())?;
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
    Ok(Args { dir, mode, token })
}

fn usage() -> String {
    "usage: txtodo-mcp --dir <workspace> --stdio|--http [--lan] [--token <id>]".to_owned()
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
    let socket = args.dir.join(SOCKET_REL);
    let agent = args.token.clone().map(|t| (t, "mcp".to_owned()));
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
