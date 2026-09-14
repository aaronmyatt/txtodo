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
use txtodo_daemon::{file_carrier, lan, relay, serve, server};
use txtodo_model::IdentityMode;
use txtodo_sync::{KeyStoreMode, Secret};

/// `txtodod --dir <workspace> [--identity-mode <tagged|sidecar>] [--key-store <auto|os|file>]
/// [--relay <url>]`; nothing is guessed from the cwd in a service.
struct Args {
    dir: PathBuf,
    /// A brand-new workspace's mode when nothing on disk is already tagged (plan decision 3);
    /// `Sidecar` when the flag is omitted (docs/questions.md Q2).
    identity_mode: IdentityMode,
    /// Which sync-keystore backend to resolve (plan M4 `sync-keystore`); `None` when the flag is
    /// omitted entirely — deliberately distinct from `Some(KeyStoreMode::Auto)`. Omitted keeps
    /// the pre-existing in-memory placeholder (`Workspace::open_with_default_mode`), so every
    /// script or test that spawns this binary without knowing about the flag is unaffected;
    /// `auto` (like `os`) touches the real OS keychain, which most CI/headless environments do
    /// not have reachable, and must never become what a plain `txtodod --dir X` does on its own.
    key_store_mode: Option<KeyStoreMode>,
    /// The relay URL (plan M8 `sync-relay-enable`, ADR 0026); `None` when `--relay` is omitted,
    /// meaning relay stays off (LAN-only, unchanged M4 behaviour) — additive, never required.
    relay_url: Option<String>,
    /// `--relay-dial-peer <hex node id>` (plan M8 `relay-converge-test`): a peer's *relay* node id
    /// to actively dial once this daemon's relay endpoint is bound, bypassing LAN discovery
    /// entirely — `relay.rs`'s module doc explains why this exists (no pairing-over-relay yet, so
    /// nothing else ever tells this daemon who to reach across a real network boundary). Test/
    /// manual-pairing-substitute only; `None` when the flag is omitted, the ordinary case.
    relay_dial_peer: Option<[u8; 32]>,
    /// `--no-lan` (plan M8 `relay-converge-test`): skips `lan::start` entirely so this daemon has
    /// no LAN path at all — proves a convergence test actually exercised the relay, not LAN
    /// discovering the same peer on a shared interface underneath it.
    no_lan: bool,
    /// `--sync-dir <path>` (plan M8 `sync-file-carrier`): the shared folder `file_carrier::start`
    /// watches; `None` when omitted, meaning the file carrier stays off — additive, like relay.
    sync_dir: Option<PathBuf>,
}

/// Lowercase (or uppercase) hex to exactly 32 bytes; `None` on anything else — `--relay-dial-peer`
/// is external input (a human or a test harness typed it), never assumed well-formed.
fn parse_relay_dial_peer(raw: &std::ffi::OsStr) -> Result<[u8; 32], String> {
    let s = raw
        .to_str()
        .ok_or_else(|| "--relay-dial-peer must be valid UTF-8 hex".to_owned())?;
    let bad = || format!("--relay-dial-peer must be 64 hex chars (32 bytes), got {s:?}");
    if s.len() != 64 {
        return Err(bad());
    }
    let mut out = [0u8; 32];
    for (i, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).map_err(|_| bad())?;
    }
    Ok(out)
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
    let mut key_store_mode = None;
    let mut relay_url = None;
    let mut relay_dial_peer = None;
    let mut no_lan = false;
    let mut sync_dir = None;
    // Bounded by the argv length; this binary's flags are all it knows.
    while let Some(a) = args.next() {
        match a.to_str() {
            Some("--dir") => dir = args.next().map(PathBuf::from),
            Some("--identity-mode") => {
                let raw = args.next().ok_or("--identity-mode needs a value")?;
                identity_mode = parse_identity_mode(&raw)?;
            }
            Some("--key-store") => {
                let raw = args.next().ok_or("--key-store needs a value")?;
                key_store_mode = Some(parse_key_store_mode(&raw)?);
            }
            Some("--relay") => {
                let raw = args.next().ok_or("--relay needs a value")?;
                relay_url = Some(raw.to_string_lossy().into_owned());
            }
            Some("--relay-dial-peer") => {
                let raw = args.next().ok_or("--relay-dial-peer needs a value")?;
                relay_dial_peer = Some(parse_relay_dial_peer(&raw)?);
            }
            Some("--no-lan") => no_lan = true,
            Some("--sync-dir") => {
                let raw = args.next().ok_or("--sync-dir needs a value")?;
                sync_dir = Some(PathBuf::from(raw));
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
        relay_url,
        relay_dial_peer,
        no_lan,
        sync_dir,
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
/// unlock via the login session). `--key-store` omitted entirely keeps the pre-existing in-memory
/// placeholder (see the field doc on `Args::key_store_mode` for why that must stay the default).
fn open_workspace(args: &Args) -> Result<Workspace, Box<dyn std::error::Error>> {
    let Some(key_store_mode) = args.key_store_mode else {
        return Ok(Workspace::open_with_default_mode(
            &args.dir,
            Arc::new(SystemClock),
            args.identity_mode,
        )?);
    };
    let file_passphrase = if key_store_mode == KeyStoreMode::File {
        Some(prompt_file_passphrase()?)
    } else {
        None
    };
    Ok(Workspace::open_with_key_store(
        &args.dir,
        Arc::new(SystemClock),
        args.identity_mode,
        key_store_mode,
        file_passphrase,
    )?)
}

/// `--no-lan` (plan M8 `relay-converge-test`): skip LAN entirely so a forced-relay test proves the
/// relay path actually carried convergence, rather than LAN quietly doing it underneath.
fn start_lan(args: &Args, ws: &server::SharedWorkspace) -> Option<lan::LanTransport> {
    if args.no_lan {
        return None;
    }
    Some(lan::start(Arc::clone(ws), Arc::new(SystemClock)))
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
    let lan_transport = start_lan(&args, &ws);
    let relay_transport = relay::start(
        Arc::clone(&ws),
        args.relay_url.clone(),
        args.relay_dial_peer,
    );
    let file_carrier_transport = file_carrier::start(Arc::clone(&ws), args.sync_dir.clone());
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
    lan_transport.as_ref().inspect(|l| l.abort());
    relay_transport.as_ref().inspect(|r| r.abort());
    file_carrier_transport.as_ref().inspect(|f| f.abort());
    eprintln!("txtodod stopped");
    Ok(())
}
