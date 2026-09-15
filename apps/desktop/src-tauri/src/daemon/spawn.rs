//! `ensure_daemon`: spawn the one device-global `txtodod` (ADR 0025, task
//! `desktop-workspace-switcher`, M11 — no `--dir`, so it opens every workspace the registry
//! already knows about) if the global socket is absent or unreachable, guarded by a client-side
//! flock so two callers in this process (or two desktop windows, or a concurrently starting CLI)
//! never race into spawning twice. The daemon's own `PidFile`
//! (`crates/txtodo-daemon/src/pidfile.rs`) is the second line of defense: a duplicate spawn just
//! exits immediately, naming the pid already holding the lock. `cfg.workspace` no longer decides
//! *which* daemon to dial — only which workspace the connected client's selector names first
//! (`commands::connect_and_store`).

#[cfg(unix)]
pub use unix_impl::ensure_daemon;

#[cfg(not(unix))]
pub use stub::ensure_daemon;

#[cfg(unix)]
mod unix_impl {
    use crate::config::{self, DesktopConfig};
    use crate::daemon::DaemonError;
    use std::fs::{File, OpenOptions};
    use std::io;
    use std::path::{Path, PathBuf};
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    /// How long to sleep between socket-liveness probes while waiting for a spawn.
    const POLL_INTERVAL: Duration = Duration::from_millis(50);

    /// Ensures the global daemon is listening on `config::global_socket_path()`, spawning
    /// `txtodod` (no `--dir`) if it is absent or unreachable (a stale socket file with no
    /// listener counts as absent — the daemon unlinks stale sockets itself on start, so this only
    /// retries, never unlinks). Returns the socket path once a live daemon answers on it.
    pub async fn ensure_daemon(cfg: &DesktopConfig) -> Result<PathBuf, DaemonError> {
        let sock = cfg.resolved_global_socket();
        if probe_live(&sock).await {
            return Ok(sock);
        }
        let _guard = SpawnGuard::acquire(&sock).await?;
        if !probe_live(&sock).await {
            spawn_txtodod(cfg)?;
            wait_until_live(&sock, cfg.spawn_timeout).await?;
        }
        Ok(sock)
    }

    /// A live listener answers a bare connect; a missing or stale socket does not.
    async fn probe_live(sock: &Path) -> bool {
        tokio::net::UnixStream::connect(sock).await.is_ok()
    }

    /// Polls [`probe_live`] until it succeeds or `timeout` elapses.
    async fn wait_until_live(sock: &Path, timeout: Duration) -> Result<(), DaemonError> {
        let start = Instant::now();
        while !probe_live(sock).await {
            if start.elapsed() >= timeout {
                return Err(DaemonError::Timeout);
            }
            tokio::time::sleep(POLL_INTERVAL).await;
        }
        Ok(())
    }

    /// Spawns `txtodod` (true global mode, no `--dir`) with a fixed argv (no shell string) and
    /// reaps it on a background thread so it never lingers as a zombie once it exits; the daemon
    /// outlives this call and is not otherwise supervised here. `global_socket_override`/
    /// `global_registry_override`, when set, ride the child's own environment — never this
    /// process' — so a hermetic test's daemon binds where the test expects without racing any
    /// other concurrently running test over a shared, mutated process environment.
    fn spawn_txtodod(cfg: &DesktopConfig) -> Result<(), DaemonError> {
        let program = cfg
            .daemon_bin
            .clone()
            .unwrap_or_else(|| PathBuf::from("txtodod"));
        let mut command = Command::new(program);
        command.stdout(Stdio::null()).stderr(Stdio::null());
        if let Some(sock) = &cfg.global_socket_override {
            command.env("TXTODO_SOCKET", sock);
        }
        if let Some(registry) = &cfg.global_registry_override {
            command.env("TXTODO_REGISTRY_DB", registry);
        }
        let mut child = command.spawn().map_err(DaemonError::Spawn)?;
        std::thread::spawn(move || {
            let _status = child.wait();
        });
        Ok(())
    }

    /// Client-side no-double-spawn guard: an exclusive lock beside `sock` (`desktop-spawn.lock`),
    /// held for the duration of the absent-check-then-spawn so two `ensure_daemon` callers never
    /// both decide to spawn a daemon for the same socket — device-global by default
    /// (`config::global_state_dir()`-adjacent, via `sock`'s own parent), or test-isolated when
    /// `global_socket_override` points `sock` somewhere else entirely.
    /// Ref: <https://doc.rust-lang.org/std/fs/struct.File.html#method.lock>
    struct SpawnGuard {
        _file: File,
    }

    impl SpawnGuard {
        async fn acquire(sock: &Path) -> Result<SpawnGuard, DaemonError> {
            let dir = sock
                .parent()
                .map_or_else(config::global_state_dir, Path::to_path_buf);
            let path = dir.join("desktop-spawn.lock");
            let file = tokio::task::spawn_blocking(move || -> io::Result<File> {
                std::fs::create_dir_all(&dir)?;
                let file = OpenOptions::new()
                    .create(true)
                    .write(true)
                    .truncate(false)
                    .open(&path)?;
                file.lock()?;
                Ok(file)
            })
            .await
            .map_err(|_join_err| DaemonError::Lock(io::Error::other("spawn lock task panicked")))?
            .map_err(DaemonError::Lock)?;
            Ok(SpawnGuard { _file: file })
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
