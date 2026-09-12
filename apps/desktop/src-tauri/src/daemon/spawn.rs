//! `ensure_daemon`: spawn `txtodod --dir <workspace>` if the socket is absent or unreachable,
//! guarded by a client-side flock so two callers in this process (or two windows on the same
//! workspace) never race into spawning twice. The daemon's own `PidFile`
//! (`crates/txtodo-daemon/src/pidfile.rs`) is the second line of defense: a duplicate spawn just
//! exits immediately, naming the pid already holding the lock.

#[cfg(unix)]
pub use unix_impl::ensure_daemon;

#[cfg(not(unix))]
pub use stub::ensure_daemon;

#[cfg(unix)]
mod unix_impl {
    use crate::config::DesktopConfig;
    use crate::daemon::DaemonError;
    use std::fs::{File, OpenOptions};
    use std::io;
    use std::path::{Path, PathBuf};
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    /// How long to sleep between socket-liveness probes while waiting for a spawn.
    const POLL_INTERVAL: Duration = Duration::from_millis(50);

    /// Ensures a daemon is listening on `cfg.socket_path()`, spawning `txtodod --dir <workspace>`
    /// if it is absent or unreachable (a stale socket file with no listener counts as absent —
    /// the daemon unlinks stale sockets itself on start, so this only retries, never unlinks).
    /// Returns the socket path once a live daemon answers on it.
    pub async fn ensure_daemon(cfg: &DesktopConfig) -> Result<PathBuf, DaemonError> {
        let sock = cfg.socket_path();
        let _guard = SpawnGuard::acquire(cfg).await?;
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

    /// Spawns `txtodod --dir <workspace>` with a fixed argv (no shell string) and reaps it on a
    /// background thread so it never lingers as a zombie once it exits; the daemon outlives this
    /// call and is not otherwise supervised here.
    fn spawn_txtodod(cfg: &DesktopConfig) -> Result<(), DaemonError> {
        let program = cfg
            .daemon_bin
            .clone()
            .unwrap_or_else(|| PathBuf::from("txtodod"));
        let mut child = Command::new(program)
            .arg("--dir")
            .arg(&cfg.workspace)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(DaemonError::Spawn)?;
        std::thread::spawn(move || {
            let _status = child.wait();
        });
        Ok(())
    }

    /// Client-side no-double-spawn guard: an exclusive lock on
    /// `<workspace>/.txtodo/desktop-spawn.lock`, held for the duration of the absent-check-then-
    /// spawn so two `ensure_daemon` callers on the same workspace never both decide to spawn.
    /// Ref: <https://doc.rust-lang.org/std/fs/struct.File.html#method.lock>
    struct SpawnGuard {
        _file: File,
    }

    impl SpawnGuard {
        async fn acquire(cfg: &DesktopConfig) -> Result<SpawnGuard, DaemonError> {
            let dir = cfg.state_dir();
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

    pub async fn ensure_daemon(_cfg: &DesktopConfig) -> Result<PathBuf, DaemonError> {
        Err(DaemonError::UnsupportedPlatform)
    }
}
