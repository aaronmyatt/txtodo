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
    /// (`DesktopConfig::DEFAULT_SPAWN_TIMEOUT`) was 30s, sized for a debug build on a slow CI
    /// runner opening a small test workspace. That's too short for a real cold start in true
    /// global mode: `open_all_registered` opens every registered workspace's documents
    /// *sequentially*, and rebuilding each one's Loro mirror from its stored snapshot
    /// (`external.rs::load_mirror`) is a known, already-measured, non-trivial per-document cost
    /// (`mirror.rs`'s own doc comment: "22s per 10k-line adopt" in a debug build). A real,
    /// organically-grown registry (this project's own dogfooded backlog: ~1700 documents across
    /// nested `tasks/*/` dirs) measured ~38s cold, in a *release* build, before the socket
    /// answered — comfortably past the old 30s ceiling, so `ensure_daemon` gave up and every
    /// daemon-mode CLI command fell back to "needs the daemon" even though the daemon was doing
    /// fine and came up moments later. 120s gives real headroom without the underlying, still-open
    /// scaling problem (sequential opens, eager full-mirror-replay per document — see
    /// `crates/txtodo-daemon/CLAUDE.md`'s `idle_rss` note for the same pipeline's memory side of
    /// this) — that's a real performance fix, not a timeout tweak, and stays out of scope here.
    pub const DEFAULT_SPAWN_TIMEOUT: Duration = Duration::from_secs(120);

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
            if already_installed_as_service(cfg) {
                // The boot unit owns this target already (ADR 0025) and should already be
                // starting on its own via `RunAtLoad`/`WantedBy` — wait for it instead of racing
                // it with a second, ad-hoc bare-PATH `txtodod` (root todo 2: seen holding the pid
                // lock right next to a still-booting boot-unit instance). No active kickstart
                // here: `launchctl kickstart -k` kills-and-restarts, which would abort an
                // already-booting instance mid-`open_all_registered` and pay its full workspace-
                // open cost twice — worse than waiting.
                wait_until_live(&cfg.socket, cfg.spawn_timeout).await?;
            } else {
                spawn_daemon(cfg)?;
                wait_until_live(&cfg.socket, cfg.spawn_timeout).await?;
                install_persistent_service_best_effort(cfg);
            }
        }
        Ok(())
    }

    /// Whether a persistent, non-stale boot-time unit is already installed for `cfg`'s target
    /// (ADR 0025: the global daemon only — `cfg.extra_args` non-empty means a legacy `--dir`
    /// bridge, which never gets one).
    ///
    /// Also `false` whenever `cfg.extra_env` is non-empty: only a hermetic test/harness sets it (to
    /// redirect the spawned child at an isolated `TXTODO_SOCKET`/`TXTODO_REGISTRY_DB`), unrelated to
    /// whatever unit is installed against the real `$HOME`. Found the hard way: without this,
    /// `tests/ensure_daemon.rs` timed out waiting on the test's own never-to-be-bound socket instead
    /// of spawning against it, on any machine with a real installed service.
    fn already_installed_as_service(cfg: &LaunchConfig) -> bool {
        if !cfg.extra_args.is_empty() || !cfg.extra_env.is_empty() {
            return false;
        }
        let Some(txtodod) = resolve_binary_path(cfg) else {
            return false;
        };
        let Ok(home) = crate::service::home_dir() else {
            return false;
        };
        already_installed_at(&home, &txtodod)
    }

    /// The pure half of [`already_installed_as_service`], `home` injected rather than read from
    /// `$HOME` — the same "inject the env-derived path" shape `service_tests.rs`'s own tests use
    /// via `render(home.path(), ...)`, needed here because this crate forbids `unsafe` so tests
    /// cannot override the real process-wide `HOME` in-process either. A *stale* installed unit
    /// (`crate::service::is_stale` — a dead binary path, or the pre-fix `KeepAlive` shape) reads
    /// as "not installed": it can never come up on its own, so falling through to the ad-hoc
    /// spawn (which then repairs it via `install_persistent_service_best_effort`) is still correct.
    fn already_installed_at(home: &Path, txtodod: &Path) -> bool {
        let Some(rendered) = crate::service::render(home, txtodod) else {
            return false;
        };
        rendered.path.exists() && !crate::service::is_stale(&rendered)
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
    /// ADR 0025 gives the one boot-time unit to the global daemon only. Also skipped whenever
    /// `extra_env` is non-empty (see [`already_installed_as_service`]'s doc: "hermetic test/
    /// harness, not a real caller") — found the hard way when an in-process `ensure_daemon` test
    /// really installed and started a persistent unit against the real `$HOME`.
    ///
    /// Also self-heals a stale existing unit (task `daemon-stale-service-repair`): one whose
    /// recorded binary path no longer exists (e.g. a git worktree removed after install) can
    /// never succeed no matter how many times `KeepAlive`/`Restart=on-failure` retries it, and
    /// nothing else ever notices — every previous caller here treated "already installed" as
    /// good enough. `crate::service::is_stale` is checked first so a merely-already-installed,
    /// still-valid unit (the common case) is never force-overwritten.
    fn install_persistent_service_best_effort(cfg: &LaunchConfig) {
        if !cfg.extra_args.is_empty() || !cfg.extra_env.is_empty() {
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
        // `force`: only when the existing unit is stale (a repair, not a customization
        // clobber) — never clobber a service file a human or a previous install already
        // customized. An "already exists" error from `install` with `force: false` is exactly
        // the idempotent no-op this needs otherwise, not a failure worth reporting.
        //
        // Only a binary the service manager can really exec is ever written: launchd/systemd
        // run with cwd `/`, so a relative or missing `daemon_bin` would just install another
        // stale unit (and re-trigger this repair on every launch). An existing unit is still
        // (re)started either way.
        if txtodod.is_absolute() && txtodod.is_file() {
            let force = crate::service::is_stale(&rendered);
            if force {
                // launchd keeps the job it loaded from the old file: without a bootout first,
                // `bootstrap` in `start` fails as "already loaded" and the dead path stays live.
                // Ref: https://keith.github.io/xcode-man-pages/launchctl.1.html
                let _ = crate::service::stop(&rendered);
            }
            let _ = crate::service::install(&home, &rendered, force);
        }
        let _ = crate::service::start(&rendered);
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

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn a_legacy_dir_bridge_target_is_never_treated_as_installed() {
            let cfg = LaunchConfig::new("/tmp/does-not-matter.sock").with_dir("/some/workspace");
            assert!(
                !already_installed_as_service(&cfg),
                "ADR 0025: only the global daemon ever gets the boot-time unit"
            );
        }

        /// The exact reported bug: a hermetic test/harness redirecting the spawned child at an
        /// isolated `TXTODO_SOCKET`/`TXTODO_REGISTRY_DB` via `extra_env` must never be told "already
        /// installed" off the real machine's real `$HOME` — that unit, if any, has nothing to do with
        /// the isolated target, so treating it as installed makes `ensure_daemon` wait forever on a
        /// socket nothing will bind (`tests/ensure_daemon.rs` deterministically timed out this way on
        /// any machine with a real installed service, before this guard existed).
        #[test]
        fn a_target_with_extra_env_is_never_treated_as_installed() {
            let mut cfg = LaunchConfig::new("/tmp/does-not-matter.sock");
            cfg.extra_env = vec![(
                "TXTODO_SOCKET".to_owned(),
                "/tmp/does-not-matter.sock".to_owned(),
            )];
            assert!(
                !already_installed_as_service(&cfg),
                "extra_env is only ever set by a hermetic test/harness redirecting the child, never \
                 by a real caller"
            );
        }

        #[test]
        fn nothing_installed_is_not_installed() {
            let home = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
            assert!(!already_installed_at(
                home.path(),
                Path::new("/bin/txtodod")
            ));
        }

        #[test]
        fn a_real_installed_unit_is_installed() {
            let home = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
            let real_bin = home.path().join("txtodod");
            std::fs::write(&real_bin, b"#!/bin/sh\n").unwrap_or_else(|e| panic!("write bin: {e}"));
            let rendered = crate::service::render(home.path(), &real_bin)
                .unwrap_or_else(|| panic!("supported platform"));
            crate::service::install(home.path(), &rendered, false)
                .unwrap_or_else(|e| panic!("install: {e}"));

            assert!(already_installed_at(home.path(), &real_bin));
        }

        #[test]
        fn a_stale_installed_unit_is_not_treated_as_installed() {
            let home = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
            let gone_bin = home.path().join("worktree-txtodod");
            std::fs::write(&gone_bin, b"#!/bin/sh\n").unwrap_or_else(|e| panic!("write bin: {e}"));
            let rendered = crate::service::render(home.path(), &gone_bin)
                .unwrap_or_else(|| panic!("supported platform"));
            crate::service::install(home.path(), &rendered, false)
                .unwrap_or_else(|e| panic!("install: {e}"));
            std::fs::remove_file(&gone_bin)
                .unwrap_or_else(|e| panic!("simulate worktree deletion: {e}"));

            assert!(
                !already_installed_at(home.path(), &gone_bin),
                "a stale unit can never come up on its own; fall through to the ad-hoc spawn \
                 (which repairs it) instead of waiting on it forever"
            );
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
