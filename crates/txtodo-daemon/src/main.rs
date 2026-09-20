//! txtodod: one process per device, owning every registered workspace's files, op log and the one
//! IPC socket (ADR 0025, task `daemon-global-socket`). Startup order: pid lock → logging →
//! registry → catalog → open workspace(s) → gRPC → "ready". SIGTERM/SIGINT stop accepting, drain,
//! remove socket.
#![forbid(unsafe_code)]
#![allow(clippy::print_stderr)] // the binary's only human output path (plan §0)
#![allow(clippy::print_stdout)] // --version's own output path (must be stdout, not stderr)

mod boot_log;
mod signals;

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;
use txtodo_daemon::args_parse::{parse_identity_mode, parse_key_store_mode, parse_relay_dial_peer};
use txtodo_daemon::clock::{Clock, SystemClock};
use txtodo_daemon::device_identity::DeviceIdentity;
use txtodo_daemon::device_relay::DeviceRelay;
use txtodo_daemon::file_carrier::DeviceFileCarrier;
use txtodo_daemon::pidfile::{PidError, PidFile};
use txtodo_daemon::runtime_exit::{SHUTDOWN_GRACE, block_on_then_shut_down};
use txtodo_daemon::serve;
use txtodo_daemon::workspace_catalog::{OpenArgs, WorkspaceCatalog};
use txtodo_daemon::workspace_registry::WorkspaceRegistry;
use txtodo_daemon::workspace_registry_paths::{self, RegistryEnv};
use txtodo_model::IdentityMode;
use txtodo_sync::{KeyStoreMode, Secret};

/// `txtodod [--dir <workspace>] [--identity-mode <tagged|sidecar>] [--key-store <auto|os|file>]
/// [--relay <url>] [--no-relay]`; nothing is guessed from the cwd. `--dir` is the legacy
/// single-workspace bridge (`tasks/daemon-global-socket/notes.md`) — omit for true global mode.
struct Args {
    /// `Some` is the legacy `--dir <workspace>` bridge; `None` is the true global mode.
    dir: Option<PathBuf>,
    /// A brand-new workspace's mode when nothing on disk is tagged; `Sidecar` if omitted.
    identity_mode: IdentityMode,
    /// Which sync-keystore backend to resolve (plan M4 `sync-keystore`); `None` (flag omitted)
    /// keeps the pre-existing in-memory placeholder — `auto`/`os` touch the real OS keychain,
    /// which most CI/headless environments can't reach.
    key_store_mode: Option<KeyStoreMode>,
    /// The explicit `--relay <url>` flag; `None` no longer means relay is off — `relay::
    /// resolve_relay_url` (this file's `run`) is the real decision now.
    relay_url: Option<String>,
    /// `--relay-dial-peer <hex node id>`: a peer's *relay* node id to dial once this daemon's
    /// relay endpoint is bound, bypassing LAN discovery. Test-only; `None` if omitted.
    relay_dial_peer: Option<[u8; 32]>,
    /// `--no-lan`: skips LAN entirely for every workspace this daemon opens (relay-only tests).
    no_lan: bool,
    /// `--no-relay`: opts out now that relay defaults on, symmetric with `--no-lan`.
    no_relay: bool,
    /// `--sync-dir <path>`: the shared folder every opened workspace's file-carrier watches;
    /// `None` (omitted) keeps it off.
    sync_dir: Option<PathBuf>,
}

/// `parse_args`'s success case: either real startup [`Args`], or `--version` asked to print and
/// exit 0 — kept distinct from the `Err` (usage error, exit 2) path below, which a bare
/// `Result<Args, String>` used to conflate: `--version` returned `Err(version_string)`, the same
/// path a real parse error takes, so `main` printed the version and still exited 2. Found on this
/// project's first real release run: the release workflow's own smoke test ("every binary runs
/// --version") failed here even though the version printed correctly.
enum ArgsOutcome {
    Run(Args),
    ShowVersion,
}

fn parse_args() -> Result<ArgsOutcome, String> {
    let mut args = std::env::args_os().skip(1);
    let mut dir: Option<PathBuf> = None;
    let mut identity_mode = IdentityMode::Sidecar;
    let mut key_store_mode = None;
    let mut relay_url = None;
    let mut relay_dial_peer = None;
    let mut no_lan = false;
    let mut no_relay = false;
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
            Some("--no-relay") => no_relay = true,
            Some("--sync-dir") => {
                let raw = args.next().ok_or("--sync-dir needs a value")?;
                sync_dir = Some(PathBuf::from(raw));
            }
            Some("--version") => return Ok(ArgsOutcome::ShowVersion),
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
    Ok(ArgsOutcome::Run(Args {
        dir,
        identity_mode,
        key_store_mode,
        relay_url,
        relay_dial_peer,
        no_lan,
        no_relay,
        sync_dir,
    }))
}

/// Reads a passphrase for `--key-store file` as one line from stdin — never a CLI argument or
/// environment variable (CLAUDE.md §3.1), so `ps`/shell history never carries it. The `String`'s
/// buffer moves directly into `Secret` (zeroized on drop) via `into_bytes`, no extra copy.
/// **Known gap**: does not suppress terminal echo (needs `rpassword`, not added without sign-off —
/// see tasks/sync-keystore/notes.md's "As built"); the CLAUDE.md §3.1 properties still hold.
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
        Ok(ArgsOutcome::Run(a)) => a,
        Ok(ArgsOutcome::ShowVersion) => {
            println!("txtodod {}", txtodo_daemon::buildinfo::VERSION_LINE);
            return ExitCode::SUCCESS;
        }
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
    match block_on_then_shut_down(rt, run(args), SHUTDOWN_GRACE) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("txtodod: {e}");
            exit_code_for(e.as_ref())
        }
    }
}

/// Losing the pid lock is not a failure: another `txtodod` is already serving, and there is
/// nothing for a supervisor to retry. Exiting 0 keeps launchd's `KeepAlive.SuccessfulExit=false`
/// and systemd's `Restart=on-failure` from respawning it forever — `launchd.err.log` held 33
/// 'already running' exits on 2026-09-19 (root todo id:01M2VV1ZXDK24H3P6Z4DJ2P8YM).
fn exit_code_for(error: &(dyn std::error::Error + 'static)) -> ExitCode {
    match error.downcast_ref::<PidError>() {
        Some(PidError::Running { .. }) => ExitCode::SUCCESS,
        _ => ExitCode::FAILURE,
    }
}

/// The state dir this run resolves to first: `<dir>/.txtodo/` for the legacy `--dir` bridge, the
/// device-global data dir for true global mode. Resolved before `WorkspaceCatalog` exists (ADR
/// 0021: the shared `DeviceIdentity` it's built from needs this path first).
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

/// Builds this process's one shared [`DeviceIdentity`] (ADR 0021) at `state_dir`, prompting once
/// for a `file`-keystore passphrase (`--key-store` omitted keeps the in-memory placeholder).
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

/// `WorkspaceOpenArgs` from the CLI flags plus the identity/relay/file-carrier `run` already
/// resolved — every workspace this catalog opens shares the identical `Option`s, never its own.
/// `relay_url` is `run`'s resolved value, not `args.relay_url` re-read (must match `device_relay`).
fn open_args(
    args: &Args,
    identity: Arc<DeviceIdentity>,
    relay_url: Option<String>,
    device_relay: Option<Arc<DeviceRelay>>,
    device_file_carrier: Option<Arc<DeviceFileCarrier>>,
) -> OpenArgs {
    OpenArgs {
        identity_mode: args.identity_mode,
        identity,
        relay_url,
        device_relay,
        relay_dial_peer: args.relay_dial_peer,
        no_lan: args.no_lan,
        device_file_carrier,
    }
}

/// The `--dir` bridge: registers/opens that one directory, plus (best-effort) anything else
/// already registered. Its socket stays at the pre-existing `<dir>/.txtodo/...` location (see
/// `run`'s `global_socket_path` call) so today's whole test suite keeps working unmodified.
fn start_dir_bridge(
    dir: &Path,
    catalog: &WorkspaceCatalog,
) -> Result<(), Box<dyn std::error::Error>> {
    eprintln!(
        "txtodod: --dir is the legacy single-workspace bridge (see \
         tasks/daemon-global-socket/notes.md); omit --dir to run this device's one global daemon"
    );
    catalog.open_dir_bridge(dir)?;
    catalog.open_all_registered();
    Ok(())
}

/// The user's default workspace: created on first run, and queued first by the caller. A failure is
/// logged and never fatal: every other workspace still opens.
fn register_default_workspace(catalog: &WorkspaceCatalog, env: &RegistryEnv) {
    let dir = workspace_registry_paths::default_workspace_dir_for(env);
    if let Err(e) = catalog.ensure_default_workspace(&dir) {
        tracing::warn!(dir = %dir.display(), error = %e, "default_workspace_unavailable");
    }
}

/// True global mode (`--dir` omitted), run once the socket is bound: opens every queued workspace
/// on the catalog's loader thread. A loader that cannot start is logged; requests still open their
/// own workspace on demand, so the daemon stays usable.
fn start_loader(
    catalog: &Arc<WorkspaceCatalog>,
    queued: Vec<(txtodo_store::WorkspaceId, PathBuf)>,
) {
    let workspaces = queued.len();
    match catalog.spawn_loader(queued) {
        Ok(_handle) => boot_log::log_loader_started(workspaces),
        Err(e) => boot_log::log_loader_failed(&e),
    }
}

/// Pid lock + log init + stale-socket removal — the very first thing `run` does, so a losing
/// second instance exits in milliseconds (before the registry, identity, relay or any workspace
/// opens: the 2026-09-19 300% CPU boot storm was four daemons each rebuilding every Loro mirror
/// before the lock told three of them to stop) and the cold-boot phase that follows lands in the
/// log. Returns the guards `run` must keep alive.
///
/// Logs go under `state_dir/logs`, **not** `workspace_registry_paths::global_log_dir(env)` called
/// directly — that always resolves the true-global location regardless of mode, so a
/// `--dir`-bridge-started daemon would silently write logs to the real machine's
/// `$XDG_DATA_HOME/txtodo/logs/` instead of `<dir>/.txtodo/logs` (a real regression this crate's
/// own daemon-slice pass caught via `tests/lan_discovery.rs`'s log-tailing assertion). `state_dir`
/// is already resolved correctly per mode by `resolve_state_dir`.
fn lock_and_start_logging(
    args: &Args,
    state_dir: &Path,
    socket: &Path,
) -> Result<(PidFile, txtodo_daemon::telemetry::LogGuard), Box<dyn std::error::Error>> {
    // The lock file lives here, and nothing else has created the directory yet.
    std::fs::create_dir_all(state_dir)?;
    let pid = PidFile::acquire(&state_dir.join("txtodod.pid"))?;
    let logs = txtodo_daemon::telemetry::init(&state_dir.join("logs"))?;
    tracing::info!(
        dir = ?args.dir.as_ref().map(|d| d.display().to_string()),
        version = txtodo_daemon::buildinfo::VERSION,
        release_date = txtodo_daemon::buildinfo::RELEASE_DATE,
        "starting"
    );
    if socket.exists() {
        // The pid lock says no other instance runs, so this is a stale socket from a crash.
        std::fs::remove_file(socket)?;
    }
    Ok((pid, logs))
}

/// Boot is pid lock → log init → `daemon_starting` → registry → identity → relay/file-carrier →
/// catalog → open workspaces → socket bind, in that order. Everything after the log init runs
/// inside one `daemon.boot` span (`start_boot_span`), entered here and dropped before the
/// long-running serve loop. JSON file always carries it; pretty stderr too in a foreground
/// terminal — launchd/systemd capture stderr into their own separate log instead
/// (`deploy/launchd/*.plist`, `deploy/systemd/txtodod.service`), no detection needed here.
async fn run(args: Args) -> Result<(), Box<dyn std::error::Error>> {
    let env = RegistryEnv::from_process()?;
    let registry_path = workspace_registry_paths::registry_db_path_for(&env, args.dir.as_deref());
    let state_dir = resolve_state_dir(&args, &env)?;
    let socket = workspace_registry_paths::global_socket_path(&env, args.dir.as_deref());
    let (_pid, _logs) = lock_and_start_logging(&args, &state_dir, &socket)?;
    let boot_span = boot_log::start_boot_span(&args);
    let _boot = boot_span.enter();
    // Not the real readiness event: that is `serve.rs::log_socket_bound`, emitted only once the
    // socket actually binds (`ref:daemon-ready-log-ordering`).
    boot_log::log_starting(&socket, &registry_path);
    let registry = WorkspaceRegistry::open(&registry_path)?;
    let clock = Arc::new(SystemClock);

    let identity = Arc::new(build_identity(&args, &state_dir, clock.as_ref())?);
    // One shared relay endpoint per device (`daemon-shared-sync-link` stage 5), bound before the
    // control channel or any workspace opens. Resolved once (defaults to a public relay unless
    // `--no-relay`) so `open_args`'s `Health` reporting below agrees with the actual bind.
    let relay_url = txtodo_daemon::relay::resolve_relay_url(args.relay_url.clone(), args.no_relay);
    let device_relay = match relay_url.clone() {
        Some(url) => DeviceRelay::bind(&identity, url).await,
        None => None,
    };
    let _control_channel = txtodo_daemon::control_channel::start(
        Arc::clone(&identity),
        device_relay.clone(),
        registry_path.clone(),
    );
    // One shared file-carrier per device (stage 6), opened before any workspace opens — every
    // workspace naming the same `--sync-dir` shares this one carrier handle.
    let device_file_carrier = args
        .sync_dir
        .clone()
        .and_then(|dir| DeviceFileCarrier::open(dir, identity.device()));
    let _file_carrier = device_file_carrier
        .clone()
        .map(txtodo_daemon::file_carrier::start);
    let catalog = Arc::new(WorkspaceCatalog::new(
        registry,
        open_args(
            &args,
            identity,
            relay_url,
            device_relay,
            device_file_carrier,
        ),
        clock,
    ));

    // The bridge opens its one directory before it binds (its whole test suite relies on that); the
    // global daemon binds first and opens registered workspaces in the background, newest first.
    let queued = match &args.dir {
        Some(dir) => {
            start_dir_bridge(dir, &catalog)?;
            Vec::new()
        }
        None => {
            register_default_workspace(&catalog, &env);
            catalog.queue_registered()
        }
    };
    drop(_boot);

    let loader_catalog = Arc::clone(&catalog);
    serve::serve_global_then(catalog, &socket, signals::shutdown_signal(), move || {
        start_loader(&loader_catalog, queued);
    })
    .await?;
    boot_log::log_stopped();
    Ok(())
}
