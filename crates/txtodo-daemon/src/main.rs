//! txtodod: one process per device, owning every registered workspace's files, op log and the one
//! IPC socket (ADR 0025, task `daemon-global-socket`). Startup order: registry → catalog → open
//! workspace(s) → pid lock → gRPC → "ready". SIGTERM/SIGINT stop accepting, drain, and remove the
//! socket and pid file.
#![forbid(unsafe_code)]
#![allow(clippy::print_stderr)] // the binary's only human output path (plan §0)

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;
use txtodo_daemon::clock::{Clock, SystemClock};
use txtodo_daemon::device_identity::DeviceIdentity;
use txtodo_daemon::pidfile::PidFile;
use txtodo_daemon::serve;
use txtodo_daemon::workspace_catalog::{OpenArgs, WorkspaceCatalog};
use txtodo_daemon::workspace_registry::WorkspaceRegistry;
use txtodo_daemon::workspace_registry_paths::{self, RegistryEnv};
use txtodo_model::IdentityMode;
use txtodo_sync::{KeyStoreMode, Secret};

/// `txtodod [--dir <workspace>] [--identity-mode <tagged|sidecar>] [--key-store <auto|os|file>]
/// [--relay <url>]`; nothing is guessed from the cwd in a service. `--dir` is the legacy
/// single-workspace bridge (`tasks/daemon-global-socket/notes.md`) — omit it to run this device's
/// one true global daemon over every workspace the registry already knows about.
struct Args {
    /// `Some` is the legacy `--dir <workspace>` bridge; `None` is the true global mode.
    dir: Option<PathBuf>,
    /// A brand-new workspace's mode when nothing on disk is already tagged (plan decision 3);
    /// `Sidecar` when the flag is omitted (docs/questions.md Q2).
    identity_mode: IdentityMode,
    /// Which sync-keystore backend to resolve (plan M4 `sync-keystore`); `None` when the flag is
    /// omitted entirely — deliberately distinct from `Some(KeyStoreMode::Auto)`. Omitted keeps
    /// the pre-existing in-memory placeholder, so every script or test that spawns this binary
    /// without knowing about the flag is unaffected; `auto` (like `os`) touches the real OS
    /// keychain, which most CI/headless environments do not have reachable.
    key_store_mode: Option<KeyStoreMode>,
    /// The relay URL (plan M8 `sync-relay-enable`, ADR 0026); `None` when `--relay` is omitted,
    /// meaning relay stays off (LAN-only, unchanged M4 behaviour) — additive, never required.
    relay_url: Option<String>,
    /// `--relay-dial-peer <hex node id>` (plan M8 `relay-converge-test`): a peer's *relay* node id
    /// to actively dial once this daemon's relay endpoint is bound, bypassing LAN discovery
    /// entirely. Test/manual-pairing-substitute only; `None` when the flag is omitted.
    relay_dial_peer: Option<[u8; 32]>,
    /// `--no-lan` (plan M8 `relay-converge-test`): skips LAN entirely for every workspace this
    /// daemon opens — proves a convergence test actually exercised the relay.
    no_lan: bool,
    /// `--sync-dir <path>` (plan M8 `sync-file-carrier`): the shared folder every opened
    /// workspace's file-carrier watches; `None` when omitted, meaning it stays off.
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
                    "unknown argument {a:?}; usage: txtodod [--dir <workspace>]"
                ));
            }
        }
    }
    let dir = dir
        .map(|d| {
            d.canonicalize()
                .map_err(|e| format!("cannot open workspace {}: {e}", d.display()))
        })
        .transpose()?;
    debug_assert!(dir.as_ref().is_none_or(|d| d.is_absolute()));
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

/// The state dir this run resolves to before anything else — the same directory that already
/// holds (or will hold) `registry.db`/`txtodod.sock`/`txtodod.pid` in whichever mode this process
/// is running: `<dir>/.txtodo/` for the legacy `--dir` bridge, the device-global data dir for true
/// global mode. Resolved *before* `WorkspaceCatalog` exists (ADR 0021: the shared `DeviceIdentity`
/// it's built from needs this path, and nothing about identity depends on the registry/catalog).
fn resolve_state_dir(
    args: &Args,
    env: &RegistryEnv,
) -> Result<PathBuf, Box<dyn std::error::Error>> {
    Ok(match &args.dir {
        Some(dir) => dir.join(txtodo_daemon::walker::STATE_DIR),
        None => workspace_registry_paths::global_pid_path(env)
            .parent()
            .map(Path::to_path_buf)
            .ok_or("cannot resolve the device-global data directory")?,
    })
}

/// Builds this process's one shared [`DeviceIdentity`] (ADR 0021, task
/// `daemon-device-set-identity`) at `state_dir`, prompting once for a `file`-keystore passphrase
/// (`--key-store` omitted entirely keeps the pre-existing in-memory placeholder — see
/// `Args::key_store_mode`'s doc for why that must stay the default).
fn build_identity(
    args: &Args,
    state_dir: &Path,
    clock: &dyn Clock,
) -> Result<DeviceIdentity, Box<dyn std::error::Error>> {
    let Some(key_store_mode) = args.key_store_mode else {
        return Ok(DeviceIdentity::open_in_memory(state_dir, clock)?);
    };
    let file_passphrase = if key_store_mode == KeyStoreMode::File {
        Some(prompt_file_passphrase()?)
    } else {
        None
    };
    Ok(DeviceIdentity::open(
        state_dir,
        clock,
        key_store_mode,
        file_passphrase,
    )?)
}

/// `WorkspaceOpenArgs` from the CLI flags plus the identity `build_identity` already resolved.
fn open_args(args: &Args, identity: Arc<DeviceIdentity>) -> OpenArgs {
    OpenArgs {
        identity_mode: args.identity_mode,
        identity,
        relay_url: args.relay_url.clone(),
        relay_dial_peer: args.relay_dial_peer,
        no_lan: args.no_lan,
        sync_dir: args.sync_dir.clone(),
    }
}

/// The `--dir` bridge: registers/opens that one directory, plus (best-effort) anything else
/// already registered — covers a human who has pointed `$TXTODO_REGISTRY_DB` at a real shared
/// registry even while still invoking `--dir`. `state_dir` is already resolved (`resolve_state_dir`);
/// returns the socket path, at its pre-existing `<dir>/.txtodo/...` location so today's whole test
/// suite keeps working unmodified.
fn start_dir_bridge(
    dir: &Path,
    env: &RegistryEnv,
    catalog: &WorkspaceCatalog,
) -> Result<PathBuf, Box<dyn std::error::Error>> {
    eprintln!(
        "txtodod: --dir is the legacy single-workspace bridge (see \
         tasks/daemon-global-socket/notes.md); omit --dir to run this device's one global daemon"
    );
    catalog.open_dir_bridge(dir)?;
    catalog.open_all_registered();
    Ok(workspace_registry_paths::global_socket_path(env, Some(dir)))
}

/// True global mode (`--dir` omitted): opens every already-registered workspace and resolves the
/// device-global socket path.
fn start_global(
    env: &RegistryEnv,
    catalog: &WorkspaceCatalog,
) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let opened = catalog.open_all_registered();
    eprintln!("txtodod: opened {opened} registered workspace(s)");
    Ok(workspace_registry_paths::global_socket_path(env, None))
}

/// Resolves until either a ctrl-c or (unix only) SIGTERM.
async fn shutdown_signal() {
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
}

/// Pid lock + log init + the "starting"/stale-socket-removal/"ready" sequence — split out of
/// `run` purely to keep that function's cognitive complexity within budget. Returns the guards
/// `run` must keep alive for its own duration.
///
/// Logs go under `state_dir/logs` — **not** `workspace_registry_paths::global_log_dir(env)`
/// called directly, a real regression this task's own daemon-slice pass introduced and its own
/// full test run caught: that always resolves the *true-global* location regardless of mode, so a
/// `--dir`-bridge-started daemon (every pre-existing test in this crate) silently wrote its logs
/// to this machine's real `$XDG_DATA_HOME/txtodo/logs/` instead of `<dir>/.txtodo/logs` — breaking
/// `tests/lan_discovery.rs`'s log-tailing assertion, which found no log at the location it
/// (correctly) expected. `state_dir` is already resolved correctly per mode by `start_dir_bridge`/
/// `start_global`, so deriving logs from it keeps both modes hermetic and in one place.
fn prepare_and_announce(
    args: &Args,
    state_dir: &std::path::Path,
    socket: &std::path::Path,
    registry_path: &std::path::Path,
) -> Result<(PidFile, txtodo_daemon::telemetry::LogGuard), Box<dyn std::error::Error>> {
    let pid = PidFile::acquire(&state_dir.join("txtodod.pid"))?;
    let logs = txtodo_daemon::telemetry::init(&state_dir.join("logs"))?;
    tracing::info!(
        dir = ?args.dir.as_ref().map(|d| d.display().to_string()),
        version = env!("CARGO_PKG_VERSION"),
        "starting"
    );
    if socket.exists() {
        // The pid lock says no other instance runs, so this is a stale socket from a crash.
        std::fs::remove_file(socket)?;
    }
    eprintln!(
        "txtodod ready: socket {}, registry {}",
        socket.display(),
        registry_path.display()
    );
    Ok((pid, logs))
}

async fn run(args: Args) -> Result<(), Box<dyn std::error::Error>> {
    let env = RegistryEnv::from_process()?;
    let registry_path = workspace_registry_paths::registry_db_path_for(&env, args.dir.as_deref());
    let registry = WorkspaceRegistry::open(&registry_path)?;
    let clock = Arc::new(SystemClock);

    let state_dir = resolve_state_dir(&args, &env)?;
    let identity = Arc::new(build_identity(&args, &state_dir, clock.as_ref())?);
    let _control_channel = txtodo_daemon::control_channel::start(
        Arc::clone(&identity),
        args.relay_url.clone(),
        registry_path.clone(),
    );
    let catalog = Arc::new(WorkspaceCatalog::new(
        registry,
        open_args(&args, identity),
        clock,
    ));

    let socket = match &args.dir {
        Some(dir) => start_dir_bridge(dir, &env, &catalog)?,
        None => start_global(&env, &catalog)?,
    };
    let (_pid, _logs) = prepare_and_announce(&args, &state_dir, &socket, &registry_path)?;

    serve::serve_global(catalog, &socket, shutdown_signal()).await?;
    eprintln!("txtodod stopped");
    Ok(())
}
