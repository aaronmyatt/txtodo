//! `txtodo daemon install|start|stop|status` (plan M3; ADR 0025 as of M11 — one global unit per
//! device, not one per workspace): a thin CLI wrapper around `txtodo_daemon_launch::service`'s
//! render/install/start/stop logic (task `daemon-always-available`, item 4 — that module now owns
//! the launchd/systemd templates, `Rendered`/`InstallOutcome`/`ServiceError` and the actual
//! `launchctl`/`systemctl` calls, shared with `ensure_daemon`'s own best-effort install). This
//! file keeps only what is genuinely CLI-specific: argument parsing, `println!`s, `CliError`
//! conversion, and `txtodod_path` (this binary's own "beside the exe, else PATH" resolution,
//! distinct from `txtodo_daemon_launch::service`'s functions, which take an already-resolved path).

use crate::client::{self, Mode, SOCKET_REL};
use crate::{CliError, Ctx};
use std::path::PathBuf;
use txtodo_daemon_launch::service::{self, ServiceError};

/// What `txtodo daemon` can do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum Action {
    /// Write the one global service file for this device, migrating any pre-M11 per-workspace
    /// units it finds.
    Install,
    /// Load and start the service.
    Start,
    /// Stop and unload the service.
    Stop,
    /// Service state and whether the socket answers.
    Status,
}

/// `txtodo_daemon_launch::service::ServiceError` -> `CliError`, preserving the same message text
/// every caller below already printed before this module delegated to that crate.
fn to_cli_err(e: ServiceError) -> CliError {
    match e {
        ServiceError::Io(e) => CliError::from(e),
        ServiceError::Message(m) => CliError::Message(m),
    }
}

fn txtodod_path() -> Result<PathBuf, CliError> {
    // Beside this binary, else whatever PATH resolves.
    let exe = std::env::current_exe().map_err(CliError::Io)?;
    let sibling = exe.with_file_name(format!("txtodod{}", std::env::consts::EXE_SUFFIX));
    Ok(if sibling.exists() {
        sibling
    } else {
        PathBuf::from("txtodod")
    })
}

/// Entry point for `txtodo daemon <action>`. One global unit (ADR 0025) — no workspace to resolve.
pub fn run(ctx: &Ctx, action: Action, force: bool) -> Result<(), CliError> {
    let home = service::home_dir().map_err(to_cli_err)?;
    let Some(rendered) = service::render(&home, &txtodod_path()?) else {
        return Err(CliError::Message(
            "txtodo daemon: not supported on this platform yet (M10)".into(),
        ));
    };
    match action {
        Action::Install => install(&home, &rendered, force),
        Action::Start => start(&rendered),
        Action::Stop => stop(&rendered),
        Action::Status => status(ctx, &rendered),
    }
}

/// Migrates any pre-M11 per-workspace units, then writes the one global unit file.
fn install(home: &std::path::Path, r: &service::Rendered, force: bool) -> Result<(), CliError> {
    let outcome = service::install(home, r, force).map_err(to_cli_err)?;
    for label in outcome.migrated {
        println!("migrated (removed) pre-M11 per-workspace unit {label}");
    }
    println!("installed {}", outcome.path.display());
    Ok(())
}

fn start(r: &service::Rendered) -> Result<(), CliError> {
    service::start(r).map_err(to_cli_err)
}

fn stop(r: &service::Rendered) -> Result<(), CliError> {
    service::stop(r).map_err(to_cli_err)
}

fn status(ctx: &Ctx, r: &service::Rendered) -> Result<(), CliError> {
    let installed = if r.path.exists() {
        "installed"
    } else {
        "not installed"
    };
    let socket = ctx.paths.dir.join(SOCKET_REL);
    let env = crate::config::Env::from_process().map_err(CliError::Io)?;
    let answers = match client::select(&ctx.paths.dir, false, &env) {
        Ok(Mode::Daemon(mut d)) => d
            .health()
            .map(|h| format!("answers ({} document(s), v{})", h.documents, h.version))
            .unwrap_or_else(|e| e.to_string()),
        Ok(Mode::Direct) => "no socket".to_owned(),
        Err(e) => e.to_string(),
    };
    println!("service {} ({installed}, {})", r.label, r.path.display());
    println!("socket {} {answers}", socket.display());
    if answers.starts_with("answers") {
        Ok(())
    } else {
        Err(CliError::Reported)
    }
}
