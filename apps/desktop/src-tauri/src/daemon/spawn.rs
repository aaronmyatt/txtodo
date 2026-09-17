//! `ensure_daemon`: spawn the one device-global `txtodod` (ADR 0025, task
//! `desktop-workspace-switcher`, M11 — no `--dir`, so it opens every workspace the registry
//! already knows about) if the global socket is absent or unreachable. Delegates to
//! `txtodo_daemon_launch::ensure_daemon` (task `daemon-always-available`, item 6) rather than
//! keeping its own probe/lock/spawn/wait copy — that crate is the one place this logic lives now,
//! shared with `txtodo-cli`/`txtodo-tui`/`txtodo-mcp`, deduping what ADR 0025's consequences
//! section already flagged as duplicated. This module now only translates between this crate's
//! own `DesktopConfig`/`DaemonError` types and `txtodo_daemon_launch`'s `LaunchConfig`/
//! `LaunchError`, so every other call site in this crate (`commands.rs`, the test suite) keeps
//! working against the same `ensure_daemon(cfg: &DesktopConfig) -> Result<PathBuf, DaemonError>`
//! signature as before.

#[cfg(unix)]
pub use unix_impl::ensure_daemon;

#[cfg(not(unix))]
pub use stub::ensure_daemon;

#[cfg(unix)]
mod unix_impl {
    use crate::config::DesktopConfig;
    use crate::daemon::DaemonError;
    use std::path::PathBuf;
    use txtodo_daemon_launch::{LaunchConfig, LaunchError};

    /// Ensures the global daemon is listening on `cfg.resolved_global_socket()`, spawning
    /// `txtodod` (no `--dir`) if it is absent or unreachable. Returns the socket path once a live
    /// daemon answers on it.
    #[tracing::instrument(name = "desktop.ensure_daemon", skip_all)]
    pub async fn ensure_daemon(cfg: &DesktopConfig) -> Result<PathBuf, DaemonError> {
        let sock = cfg.resolved_global_socket();
        let launch = to_launch_config(cfg, &sock);
        txtodo_daemon_launch::ensure_daemon(&launch)
            .await
            .map_err(map_launch_err)?;
        Ok(sock)
    }

    /// `global_socket_override`/`global_registry_override`, when set, ride the spawned child's
    /// own environment (`LaunchConfig::extra_env`) — never this process' — so a hermetic test's
    /// daemon binds where the test expects without racing any other concurrently running test
    /// over a shared, mutated process environment. This is the true global-daemon shape (empty
    /// `extra_args`), matching what `ensure_daemon`'s own best-effort persistent-service install
    /// expects.
    fn to_launch_config(cfg: &DesktopConfig, sock: &std::path::Path) -> LaunchConfig {
        let mut launch = LaunchConfig::new(sock);
        launch.daemon_bin = cfg.daemon_bin.clone();
        launch.spawn_timeout = cfg.spawn_timeout;
        if let Some(s) = &cfg.global_socket_override {
            launch
                .extra_env
                .push(("TXTODO_SOCKET".to_owned(), s.display().to_string()));
        }
        if let Some(r) = &cfg.global_registry_override {
            launch
                .extra_env
                .push(("TXTODO_REGISTRY_DB".to_owned(), r.display().to_string()));
        }
        launch
    }

    fn map_launch_err(e: LaunchError) -> DaemonError {
        match e {
            LaunchError::Spawn(e) => DaemonError::Spawn(e),
            LaunchError::Lock(e) => DaemonError::Lock(e),
            LaunchError::Timeout => DaemonError::Timeout,
            LaunchError::UnsupportedPlatform => DaemonError::UnsupportedPlatform,
        }
    }
}

/// Stub for non-unix targets; plan M10 adds the Windows named-pipe transport.
#[cfg(not(unix))]
mod stub {
    use crate::config::DesktopConfig;
    use crate::daemon::DaemonError;
    use std::path::PathBuf;

    /// Always fails: no transport is wired up for this platform yet (see the module doc).
    pub async fn ensure_daemon(_cfg: &DesktopConfig) -> Result<PathBuf, DaemonError> {
        Err(DaemonError::UnsupportedPlatform)
    }
}
