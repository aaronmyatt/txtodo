//! The unix half of task `daemon-auto-upgrade`: stop the older running daemon and bring up the
//! newer binary in its place. Reuses `spawn.rs`'s own probe/lock/spawn/wait pieces and
//! `service.rs`'s unit control, so the restart is the same thing `txtodo daemon install --force`
//! then `txtodo daemon start` does by hand, just triggered by a client that noticed the gap.

use std::fs::OpenOptions;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

use crate::spawn::unix_impl::{
    SpawnGuard, already_installed_as_service, install_persistent_service_best_effort, probe_live,
    spawn_daemon, wait_until_live,
};
use crate::spawn::{Ensured, LaunchConfig, LaunchError};
use crate::upgrade::{binary_version, decide, running_version};

/// How long a terminated daemon gets to close its socket — its graceful shutdown drains open
/// workspaces and really does take seconds (`service.rs`'s `BOOTOUT_SETTLE` has the same story).
const SHUTDOWN_WAIT: Duration = Duration::from_secs(30);
const SHUTDOWN_POLL: Duration = Duration::from_millis(100);

/// The socket is live. Restarts the daemon behind it when `cfg.upgrade_to` and the resolved
/// binary are both newer than what runs; otherwise reports it live and leaves it alone. Only the
/// global shape (no `--dir`): a legacy bridge daemon is whoever started it's to restart.
pub(crate) async fn upgrade_if_older(cfg: &LaunchConfig) -> Result<Ensured, LaunchError> {
    let Some(client) = cfg.upgrade_to.as_deref() else {
        return Ok(Ensured::AlreadyLive);
    };
    if !cfg.extra_args.is_empty() {
        return Ok(Ensured::AlreadyLive);
    }
    let Some(plan) = plan_restart(cfg, client) else {
        return Ok(Ensured::AlreadyLive);
    };
    let _guard = SpawnGuard::acquire(&cfg.socket).await?;
    // Another client may have restarted it while this one waited for the lock.
    if !probe_live(&cfg.socket).await || plan_restart(cfg, client).is_none() {
        return Ok(Ensured::AlreadyLive);
    }
    if already_installed_as_service(cfg) {
        restart_service(cfg).await?;
    } else {
        restart_adhoc(cfg).await?;
    }
    Ok(Ensured::Upgraded {
        from: plan.0,
        to: plan.1,
    })
}

/// `(running, binary)` versions when a restart is due; `None` otherwise. The client's own
/// version is compared first (a string read), the binary's `--version` only runs when the
/// client already looks newer — the common same-build case costs one file read.
fn plan_restart(cfg: &LaunchConfig, client: &str) -> Option<(String, String)> {
    let running = running_version(&cfg.socket)?;
    if !decide(&running, client, Some(client)) {
        return None;
    }
    let bin = crate::binary_path::resolve_binary(cfg.daemon_bin.as_ref())?;
    let binary = binary_version(&bin)?;
    decide(&running, client, Some(&binary)).then_some((running, binary))
}

/// The loaded boot unit owns this daemon: unload it, repoint the unit at the newer binary (a
/// forced reinstall, what `is_stale` alone would not trigger since the old binary still exists),
/// and start it again.
async fn restart_service(cfg: &LaunchConfig) -> Result<(), LaunchError> {
    let (Some(txtodod), Ok(home)) = (
        crate::binary_path::resolve_binary(cfg.daemon_bin.as_ref()),
        crate::service::home_dir(),
    ) else {
        return Err(LaunchError::Spawn(std::io::Error::other(
            "no txtodod binary or $HOME to reinstall the service with",
        )));
    };
    let Some(rendered) = crate::service::render(&home, &txtodod) else {
        return Err(LaunchError::UnsupportedPlatform);
    };
    let service_err =
        |e: crate::service::ServiceError| LaunchError::Spawn(std::io::Error::other(e));
    crate::service::stop(&rendered).map_err(service_err)?;
    crate::service::install(&home, &rendered, true).map_err(service_err)?;
    crate::service::start(&rendered).map_err(service_err)?;
    wait_until_live(&cfg.socket, cfg.spawn_timeout).await
}

/// An ad-hoc daemon (no unit, or a unit that does not own this socket): SIGTERM the pid the pid
/// file names, wait for the socket to close, then the ordinary spawn path.
async fn restart_adhoc(cfg: &LaunchConfig) -> Result<(), LaunchError> {
    let dir = cfg.socket.parent().unwrap_or_else(|| Path::new("."));
    if !terminate_locked_pid(&dir.join("txtodod.pid")) {
        return Err(LaunchError::Spawn(std::io::Error::other(
            "the running daemon's pid file is not locked; not signalling a stale pid",
        )));
    }
    wait_until_dead(&cfg.socket).await?;
    spawn_daemon(cfg)?;
    wait_until_live(&cfg.socket, cfg.spawn_timeout).await?;
    install_persistent_service_best_effort(cfg);
    Ok(())
}

/// Sends SIGTERM to the pid in `pid_file`, but only while a process still holds the file's
/// advisory lock (`txtodod`'s `PidFile` keeps it for its whole life): a pid left behind by a
/// crash may belong to anything by now and is never signalled. `true` when a signal was sent.
/// Ref: <https://doc.rust-lang.org/std/fs/struct.File.html#method.try_lock>
fn terminate_locked_pid(pid_file: &Path) -> bool {
    let Ok(file) = OpenOptions::new().read(true).write(true).open(pid_file) else {
        return false;
    };
    if !matches!(file.try_lock(), Err(std::fs::TryLockError::WouldBlock)) {
        return false; // nobody holds it (the lock this call just took drops with `file`)
    }
    let Some(pid) = std::fs::read_to_string(pid_file)
        .ok()
        .and_then(|t| t.trim().parse::<u32>().ok())
    else {
        return false;
    };
    Command::new("kill")
        .args(["-TERM", &pid.to_string()])
        .status()
        .is_ok_and(|s| s.success())
}

async fn wait_until_dead(sock: &Path) -> Result<(), LaunchError> {
    let start = std::time::Instant::now();
    while probe_live(sock).await {
        if start.elapsed() >= SHUTDOWN_WAIT {
            return Err(LaunchError::Timeout);
        }
        tokio::time::sleep(SHUTDOWN_POLL).await;
    }
    Ok(())
}
