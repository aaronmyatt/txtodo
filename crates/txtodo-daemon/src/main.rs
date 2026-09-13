//! txtodod: one process per workspace owning the files, the op log and the IPC socket (plan M3).
//! Startup order: pid lock → store → walk → actors → watcher → gRPC → "ready". SIGTERM/SIGINT
//! stop accepting, drain, and remove the socket and pid file.
#![forbid(unsafe_code)]
#![allow(clippy::print_stderr)] // the binary's only human output path (plan §0)

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::{Arc, RwLock};
use txtodo_daemon::clock::SystemClock;
use txtodo_daemon::pidfile::PidFile;
use txtodo_daemon::watch_task;
use txtodo_daemon::workspace::Workspace;
use txtodo_daemon::{lan, serve, server};
use txtodo_model::IdentityMode;

/// `txtodod --dir <workspace> [--identity-mode <tagged|sidecar>]`; nothing is guessed from the
/// cwd in a service.
struct Args {
    dir: PathBuf,
    /// A brand-new workspace's mode when nothing on disk is already tagged (plan decision 3);
    /// `Sidecar` when the flag is omitted (docs/questions.md Q2).
    identity_mode: IdentityMode,
}

fn parse_identity_mode(raw: &std::ffi::OsStr) -> Result<IdentityMode, String> {
    match raw.to_str() {
        Some("tagged") => Ok(IdentityMode::Tagged),
        Some("sidecar") => Ok(IdentityMode::Sidecar),
        _ => Err(format!(
            "--identity-mode must be tagged or sidecar, got {raw:?}"
        )),
    }
}

fn parse_args() -> Result<Args, String> {
    let mut args = std::env::args_os().skip(1);
    let mut dir: Option<PathBuf> = None;
    let mut identity_mode = IdentityMode::Sidecar;
    // Bounded by the argv length; three flags are all this binary knows.
    while let Some(a) = args.next() {
        match a.to_str() {
            Some("--dir") => dir = args.next().map(PathBuf::from),
            Some("--identity-mode") => {
                let raw = args.next().ok_or("--identity-mode needs a value")?;
                identity_mode = parse_identity_mode(&raw)?;
            }
            Some("--version") => return Err(format!("txtodod {}", env!("CARGO_PKG_VERSION"))),
            _ => {
                return Err(format!(
                    "unknown argument {a:?}; usage: txtodod --dir <workspace>"
                ));
            }
        }
    }
    let dir = dir.ok_or_else(|| "usage: txtodod --dir <workspace>".to_owned())?;
    let dir = dir
        .canonicalize()
        .map_err(|e| format!("cannot open workspace {}: {e}", dir.display()))?;
    debug_assert!(dir.is_absolute());
    Ok(Args { dir, identity_mode })
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
            eprintln!("txtodod: {e}");
            ExitCode::FAILURE
        }
    }
}

async fn run(args: Args) -> Result<(), Box<dyn std::error::Error>> {
    let state_dir = args.dir.join(txtodo_daemon::walker::STATE_DIR);
    std::fs::create_dir_all(&state_dir)?;
    let _pid = PidFile::acquire(&state_dir.join("txtodod.pid"))?;
    let _logs = txtodo_daemon::telemetry::init(&state_dir.join("logs"))?;
    tracing::info!(workspace = %args.dir.display(), version = env!("CARGO_PKG_VERSION"), "starting");
    let socket = state_dir.join("txtodod.sock");
    if socket.exists() {
        // The pid lock says no other instance runs, so this is a stale socket from a crash.
        std::fs::remove_file(&socket)?;
    }
    let ws =
        Workspace::open_with_default_mode(&args.dir, Arc::new(SystemClock), args.identity_mode)?;
    let documents = ws.paths().count();
    let ws: server::SharedWorkspace = Arc::new(RwLock::new(ws));
    let (_watcher, watch_handle) = watch_task::start(Arc::clone(&ws), Arc::new(SystemClock))?;
    let lan_transport = lan::start(Arc::clone(&ws), Arc::new(SystemClock));
    eprintln!(
        "txtodod ready: {documents} document(s), socket {}",
        socket.display()
    );
    let shutdown = async {
        let ctrl_c = tokio::signal::ctrl_c();
        #[cfg(unix)]
        {
            let mut term =
                match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
                    Ok(s) => s,
                    Err(_) => {
                        let _ = ctrl_c.await;
                        return;
                    }
                };
            tokio::select! { _ = ctrl_c => {}, _ = term.recv() => {} }
        }
        #[cfg(not(unix))]
        {
            let _ = ctrl_c.await;
        }
    };
    serve::serve(ws, &socket, shutdown).await?;
    watch_handle.abort();
    lan_transport.abort();
    eprintln!("txtodod stopped");
    Ok(())
}
