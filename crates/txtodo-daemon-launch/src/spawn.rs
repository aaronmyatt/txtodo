//! [`ensure_daemon`]: the shared spawn-if-absent implementation, extracted out of
//! `apps/desktop/src-tauri/src/daemon/spawn.rs` (that module now delegates here — task
//! `daemon-always-available`, item 6). Every caller supplies its own target socket, daemon
//! binary override and extra argv (`--dir <workspace>` for a legacy per-workspace bridge daemon,
//! or nothing for the ADR 0025 global daemon) via [`LaunchConfig`] — this module has no
//! `txtodo-daemon` dependency, only `Command::spawn` and a unix-socket liveness probe.

use std::path::PathBuf;
use std::time::Duration;

/// What to spawn and how long to wait, once [`ensure_daemon`] decides `socket` has no live
/// listener.
#[derive(Debug, Clone)]
pub struct LaunchConfig {
    /// The socket [`ensure_daemon`] probes and waits on.
    pub socket: PathBuf,
    /// `txtodod` binary to spawn; `None` resolves it on `$PATH`, like a normal install.
    pub daemon_bin: Option<PathBuf>,
    /// Extra argv for the spawned daemon: `["--dir", "<workspace>"]` for a legacy per-workspace
    /// bridge daemon (what `txtodo-tui` dials today), or empty for the ADR 0025 global daemon
    /// (what `txtodo-cli`, `txtodo-mcp --global` and `apps/desktop` dial).
    pub extra_args: Vec<String>,
    /// Extra environment variables for the spawned child only — never this process' own
    /// environment (a `Command`'s env is never shared with its parent).
    pub extra_env: Vec<(String, String)>,
    /// Upper bound on waiting for the socket to appear after spawning.
    pub spawn_timeout: Duration,
}

impl LaunchConfig {
    /// The default spawn timeout every client used before this crate existed
    /// (`DesktopConfig::DEFAULT_SPAWN_TIMEOUT`): generous enough for a debug build on a slow CI
    /// runner.
    pub const DEFAULT_SPAWN_TIMEOUT: Duration = Duration::from_secs(30);

    /// A config for `socket` with every other field defaulted: no override binary, no extra
    /// argv/env (global-daemon shape), the default spawn timeout.
    pub fn new(socket: impl Into<PathBuf>) -> LaunchConfig {
        LaunchConfig {
            socket: socket.into(),
            daemon_bin: None,
            extra_args: Vec::new(),
            extra_env: Vec::new(),
            spawn_timeout: LaunchConfig::DEFAULT_SPAWN_TIMEOUT,
        }
    }

    /// Targets a legacy per-workspace bridge daemon (`txtodod --dir <workspace>`) instead of the
    /// global one — what [`ensure_daemon`]'s best-effort service install skips (ADR 0025: only
    /// the one global daemon owns the boot-time unit).
    #[must_use]
    pub fn with_dir(mut self, workspace: impl AsRef<std::path::Path>) -> LaunchConfig {
        self.extra_args = vec!["--dir".to_owned(), workspace.as_ref().display().to_string()];
        self
    }
}

/// Everything that can go wrong ensuring a daemon exists.
#[derive(Debug)]
pub enum LaunchError {
    /// `txtodod` could not be spawned.
    Spawn(std::io::Error),
    /// The client-side no-double-spawn lock could not be taken.
    Lock(std::io::Error),
    /// Waiting for the socket to answer ran past `spawn_timeout`.
    Timeout,
    /// This platform has no unix-domain-socket transport yet.
    UnsupportedPlatform,
}

impl std::fmt::Display for LaunchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LaunchError::Spawn(e) => write!(f, "spawn txtodod: {e}"),
            LaunchError::Lock(e) => write!(f, "spawn lock: {e}"),
            LaunchError::Timeout => write!(f, "daemon did not become ready in time"),
            LaunchError::UnsupportedPlatform => write!(f, "unix sockets only for now"),
        }
    }
}

impl std::error::Error for LaunchError {}

#[cfg(not(unix))]
pub use stub::ensure_daemon;
#[cfg(unix)]
pub use unix_impl::ensure_daemon;

#[cfg(unix)]
mod unix_impl {
    use super::{LaunchConfig, LaunchError};
    use std::fs::{File, OpenOptions};
    use std::io;
    use std::path::{Path, PathBuf};
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    /// How long to sleep between socket-liveness probes while waiting for a spawn.
    const POLL_INTERVAL: Duration = Duration::from_millis(50);

    /// Ensures a live daemon answers on `cfg.socket`, spawning one (and, best-effort, installing
    /// it as the persistent boot service) if absent. Returns once the socket answers. No
    /// `tracing` span here (unlike `apps/desktop`'s own copy) — this crate stays dependency-free
    /// beyond `tokio` so `txtodo-mcp`/`txtodo-tui` (whose `allowedDeps` lists are deliberately
    /// short) can depend on it cheaply; callers that want a span wrap this call themselves.
    pub async fn ensure_daemon(cfg: &LaunchConfig) -> Result<(), LaunchError> {
        if probe_live(&cfg.socket).await {
            return Ok(());
        }
        let _guard = SpawnGuard::acquire(&cfg.socket).await?;
        if !probe_live(&cfg.socket).await {
            spawn_daemon(cfg)?;
            wait_until_live(&cfg.socket, cfg.spawn_timeout).await?;
            install_persistent_service_best_effort(cfg);
        }
        Ok(())
    }

    async fn probe_live(sock: &Path) -> bool {
        tokio::net::UnixStream::connect(sock).await.is_ok()
    }

    async fn wait_until_live(sock: &Path, timeout: Duration) -> Result<(), LaunchError> {
        let start = Instant::now();
        while !probe_live(sock).await {
            if start.elapsed() >= timeout {
                return Err(LaunchError::Timeout);
            }
            tokio::time::sleep(POLL_INTERVAL).await;
        }
        Ok(())
    }

    /// Spawns `txtodod` with a fixed argv (no shell string) and reaps it on a background thread
    /// so it never lingers as a zombie once it exits; the daemon outlives this call.
    fn spawn_daemon(cfg: &LaunchConfig) -> Result<(), LaunchError> {
        let program = cfg
            .daemon_bin
            .clone()
            .unwrap_or_else(|| PathBuf::from("txtodod"));
        let mut command = Command::new(program);
        command.stdout(Stdio::null()).stderr(Stdio::null());
        command.args(&cfg.extra_args);
        for (key, value) in &cfg.extra_env {
            command.env(key, value);
        }
        let mut child = command.spawn().map_err(LaunchError::Spawn)?;
        std::thread::spawn(move || {
            let _status = child.wait();
        });
        Ok(())
    }

    /// Best-effort, non-fatal: install+start the persistent boot service (reusing
    /// [`crate::service`]'s launchd/systemd logic) so a *subsequent* reboot recovers this daemon
    /// without another ad-hoc spawn. Never surfaces its own failure — the ad-hoc spawn above
    /// already succeeded, and a sandboxed/CI environment with no launchd/systemd (or no `$HOME`)
    /// is expected to fail here quietly (task item 2: "best-effort and non-fatal"). Skipped
    /// entirely for a non-global target (`extra_args` non-empty, i.e. a legacy `--dir` bridge):
    /// ADR 0025 gives the one boot-time unit to the global daemon only.
    fn install_persistent_service_best_effort(cfg: &LaunchConfig) {
        if !cfg.extra_args.is_empty() {
            return;
        }
        let Some(txtodod) = resolve_binary_path(cfg) else {
            return;
        };
        let Ok(home) = crate::service::home_dir() else {
            return;
        };
        let Some(rendered) = crate::service::render(&home, &txtodod) else {
            return;
        };
        // `force: false`: never clobber a service file a human or a previous install already
        // customized. An "already exists" error from `install` is exactly the idempotent no-op
        // this needs, not a failure worth reporting.
        if crate::service::install(&home, &rendered, false).is_ok() {
            let _ = crate::service::start(&rendered);
        } else {
            let _ = crate::service::start(&rendered); // already installed; (re)start is still useful
        }
    }

    fn resolve_binary_path(cfg: &LaunchConfig) -> Option<PathBuf> {
        if let Some(bin) = &cfg.daemon_bin {
            return Some(bin.clone());
        }
        which_on_path("txtodod")
    }

    /// The same `$PATH` search a bare `Command::new("txtodod")` performs, needed here because the
    /// rendered service file wants a concrete path, not a bare program name.
    fn which_on_path(program: &str) -> Option<PathBuf> {
        std::env::var_os("PATH").and_then(|paths| {
            std::env::split_paths(&paths)
                .map(|dir| dir.join(program))
                .find(|p| p.is_file())
        })
    }

    /// Client-side no-double-spawn guard: an exclusive lock beside `sock`, held for the duration
    /// of the absent-check-then-spawn so two `ensure_daemon` callers (in this process or another)
    /// never both decide to spawn a daemon for the same socket.
    /// Ref: <https://doc.rust-lang.org/std/fs/struct.File.html#method.lock>
    struct SpawnGuard {
        _file: File,
    }

    impl SpawnGuard {
        async fn acquire(sock: &Path) -> Result<SpawnGuard, LaunchError> {
            let dir = sock
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_else(std::env::temp_dir);
            let path = dir.join("daemon-launch-spawn.lock");
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
            .map_err(|_join_err| LaunchError::Lock(io::Error::other("spawn lock task panicked")))?
            .map_err(LaunchError::Lock)?;
            Ok(SpawnGuard { _file: file })
        }
    }
}

/// Stub for non-unix targets; matches `apps/desktop`'s own stub (Windows named-pipe transport is
/// a later milestone).
#[cfg(not(unix))]
mod stub {
    use super::{LaunchConfig, LaunchError};

    /// Always fails: no transport is wired up for this platform yet.
    pub async fn ensure_daemon(_cfg: &LaunchConfig) -> Result<(), LaunchError> {
        Err(LaunchError::UnsupportedPlatform)
    }
}
