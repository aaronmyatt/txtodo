//! txtodod: one process per workspace owning the files, the op log and the IPC socket (plan M3).
//! Startup order: pid lock → store → walk → actors → watcher → gRPC → "ready". SIGTERM/SIGINT
//! stop accepting, drain, and remove the socket and pid file.
#![forbid(unsafe_code)]
#![allow(clippy::print_stderr)] // the binary's only human output path (plan §0)

use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::{Arc, RwLock};
use txtodo_daemon::clock::SystemClock;
use txtodo_daemon::pidfile::PidFile;
use txtodo_daemon::watch_task;
use txtodo_daemon::workspace::Workspace;
use txtodo_daemon::{serve, server};
use txtodo_model::IdentityMode;
use txtodo_sync::{KeyStoreMode, Secret};

/// `txtodod --dir <workspace> [--identity-mode <tagged|sidecar>] [--key-store <auto|os|file>]`;
/// nothing is guessed from the cwd in a service.
struct Args {
    dir: PathBuf,
    /// A brand-new workspace's mode when nothing on disk is already tagged (plan decision 3);
    /// `Sidecar` when the flag is omitted (docs/questions.md Q2).
    identity_mode: IdentityMode,
    /// Which sync-keystore backend to resolve (plan M4 `sync-keystore`); `Auto` when omitted.
    key_store_mode: KeyStoreMode,
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

fn parse_key_store_mode(raw: &std::ffi::OsStr) -> Result<KeyStoreMode, String> {
    match raw.to_str() {
        Some("auto") => Ok(KeyStoreMode::Auto),
        Some("os") => Ok(KeyStoreMode::Os),
        Some("file") => Ok(KeyStoreMode::File),
        _ => Err(format!("--key-store must be auto, os or file, got {raw:?}")),
    }
}

fn parse_args() -> Result<Args, String> {
    let mut args = std::env::args_os().skip(1);
    let mut dir: Option<PathBuf> = None;
    let mut identity_mode = IdentityMode::Sidecar;
    let mut key_store_mode = KeyStoreMode::Auto;
    // Bounded by the argv length; four flags are all this binary knows.
    while let Some(a) = args.next() {
        match a.to_str() {
            Some("--dir") => dir = args.next().map(PathBuf::from),
            Some("--identity-mode") => {
                let raw = args.next().ok_or("--identity-mode needs a value")?;
                identity_mode = parse_identity_mode(&raw)?;
            }
            Some("--key-store") => {
                let raw = args.next().ok_or("--key-store needs a value")?;
                key_store_mode = parse_key_store_mode(&raw)?;
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
    Ok(Args {
        dir,
        identity_mode,
        key_store_mode,
    })
}

/// Reads a passphrase for `--key-store file` as one line from stdin — never a CLI argument or
/// environment variable (CLAUDE.md §3.1), so `ps`/shell history never carries it. The `String`'s
/// buffer moves directly into `Secret` (zeroized on drop) via `into_bytes`, no extra copy.
///
/// **Known gap, flagged for the human**: this does not suppress terminal echo. Doing so needs a
/// terminal-control dependency (e.g. `rpassword`) this pass did not add without sign-off — see
/// tasks/sync-keystore/notes.md's "As built" section. The two properties CLAUDE.md §3.1 actually
/// requires — never a CLI arg, never an env var, never logged — hold regardless.
fn prompt_file_passphrase() -> Result<Secret, Box<dyn std::error::Error>> {
    eprint!("txtodod: key_store = \"file\" passphrase: ");
    std::io::stderr().flush().ok();
    let mut line = String::new();
    std::io::stdin().read_line(&mut line)?;
    let kept = line.trim_end_matches(['\n', '\r']).len();
    line.truncate(kept);
    if line.is_empty() {
        return Err("no passphrase read from stdin; key_store = \"file\" needs one".into());
    }
    Ok(Secret::new(line.into_bytes()))
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

/// `file` needs a passphrase (prompted here, on this binary's own stdin — never a CLI argument or
/// environment variable, CLAUDE.md §3.1); `auto`/`os` never do (the OS keychain manages its own
/// unlock via the login session).
fn open_workspace(args: &Args) -> Result<Workspace, Box<dyn std::error::Error>> {
    let file_passphrase = if args.key_store_mode == KeyStoreMode::File {
        Some(prompt_file_passphrase()?)
    } else {
        None
    };
    Ok(Workspace::open_with_key_store(
        &args.dir,
        Arc::new(SystemClock),
        args.identity_mode,
        args.key_store_mode,
        file_passphrase,
    )?)
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
    let ws = open_workspace(&args)?;
    let documents = ws.paths().count();
    let key_store_backend = ws.key_store_backend_name();
    let ws: server::SharedWorkspace = Arc::new(RwLock::new(ws));
    let (_watcher, watch_handle) = watch_task::start(Arc::clone(&ws), Arc::new(SystemClock))?;
    eprintln!(
        "txtodod ready: {documents} document(s), socket {}, key_store={key_store_backend}",
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
    eprintln!("txtodod stopped");
    Ok(())
}
