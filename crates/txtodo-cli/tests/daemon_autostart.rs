//! Real-`txtodod` acceptance test for task `daemon-always-available`'s CLI wiring
//! (`main.rs::daemon_ensure`): a `NEEDS_DAEMON` command (`log`) run against a workspace with no
//! daemon running and no socket anywhere still succeeds, because `run()`'s ensure-then-retry path
//! spawns the global `txtodod` on demand instead of erroring immediately. No `txtodo daemon
//! start` is ever run manually.
//!
//! Hermetic: `TXTODO_SOCKET`/`TXTODO_REGISTRY_DB` point into a fresh temp dir instead of this
//! machine's real `$XDG_DATA_HOME/txtodo/`, and `HOME` is overridden too so `ensure_daemon`'s own
//! best-effort persistent-service-install step (`txtodo_daemon_launch::service`) never touches
//! this machine's real launchd/systemd user config — every plist/unit it might write lands under
//! the overridden `$HOME` instead. Production `ensure_daemon` resolves a bare `txtodod` on
//! `$PATH` (`LaunchConfig::daemon_bin` stays `None`; `main.rs`'s own wiring has no test-only
//! override hook), so the real, unmodified code path is exercised by building `txtodod` on demand
//! (mirroring `tests/daemon_mode.rs::txtodod_binary`) and prepending its directory to the child
//! `txtodo` process's own `$PATH`, rather than pointing at it directly.
#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// `target/debug/deps/<test>` -> `target/debug`; builds `txtodod` there on demand (same as
/// `tests/daemon_mode.rs::txtodod_binary`) and returns the directory holding it, so it can be
/// prepended to a child process' own `$PATH`.
fn txtodod_dir() -> PathBuf {
    let mut dir = std::env::current_exe().unwrap_or_else(|e| panic!("current_exe: {e}"));
    dir.pop();
    if dir.ends_with("deps") {
        dir.pop();
    }
    let bin = dir.join(format!("txtodod{}", std::env::consts::EXE_SUFFIX));
    if !bin.exists() {
        let status = Command::new(env!("CARGO"))
            .args([
                "build",
                "-p",
                "txtodo-daemon",
                "--bin",
                "txtodod",
                "--quiet",
            ])
            .status()
            .unwrap_or_else(|e| panic!("cargo build txtodod: {e}"));
        assert!(status.success(), "building txtodod failed");
    }
    assert!(bin.exists(), "{}", bin.display());
    dir
}

/// Runs `txtodo` against a hermetic global-daemon home: no per-dir socket, and `TXTODO_SOCKET`/
/// `TXTODO_REGISTRY_DB`/`HOME` all point into `state_dir`/`home` rather than the real machine.
fn run_txtodo(workspace: &Path, state_dir: &Path, home: &Path, args: &[&str]) -> Output {
    let path_with_txtodod = std::env::join_paths(
        std::iter::once(txtodod_dir()).chain(
            std::env::var_os("PATH")
                .map(|p| std::env::split_paths(&p).collect::<Vec<_>>())
                .unwrap_or_default(),
        ),
    )
    .unwrap_or_else(|e| panic!("join_paths: {e}"));
    Command::new(env!("CARGO_BIN_EXE_txtodo"))
        .current_dir(workspace)
        .env_remove("TXTODO_TODO_DIR")
        .env("TXTODO_CONFIG", workspace.join("none.toml"))
        .env("TXTODO_SOCKET", state_dir.join("txtodod.sock"))
        .env("TXTODO_REGISTRY_DB", state_dir.join("registry.db"))
        .env("HOME", home)
        .env("PATH", path_with_txtodod)
        .args(args)
        .output()
        .unwrap_or_else(|e| panic!("spawn txtodo: {e}"))
}

#[test]
fn log_cold_starts_the_global_daemon_with_no_manual_daemon_start() {
    let workspace = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
    std::fs::write(workspace.path().join("todo.txt"), "buy milk\n")
        .unwrap_or_else(|e| panic!("write todo.txt: {e}"));
    let state_dir = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
    let home = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));

    // No socket exists anywhere yet, and this test never runs `txtodo daemon start`: `log` (a
    // NEEDS_DAEMON command, `main.rs::daemon_ensure::needs_daemon`) must still succeed.
    let out = run_txtodo(
        workspace.path(),
        state_dir.path(),
        home.path(),
        &["log", "-n", "5"],
    );
    assert!(
        out.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let socket = state_dir.path().join("txtodod.sock");
    assert!(
        socket.exists(),
        "ensure_daemon should have spawned the global daemon and left its socket behind"
    );

    // A second invocation must reuse the now-running daemon rather than spawning a second one:
    // the pid file's contents (the daemon's own single-instance lock, `PidFile::acquire`) must be
    // identical across both calls.
    let pid_path = state_dir.path().join("txtodod.pid");
    let pid_before =
        std::fs::read_to_string(&pid_path).unwrap_or_else(|e| panic!("read pid file: {e}"));
    let out2 = run_txtodo(
        workspace.path(),
        state_dir.path(),
        home.path(),
        &["log", "-n", "5"],
    );
    assert!(
        out2.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&out2.stdout),
        String::from_utf8_lossy(&out2.stderr)
    );
    let pid_after =
        std::fs::read_to_string(&pid_path).unwrap_or_else(|e| panic!("read pid file: {e}"));
    assert_eq!(
        pid_before, pid_after,
        "exactly one txtodod, not a second spawn"
    );

    if let Ok(pid) = pid_after.trim().parse::<u32>() {
        let _ = Command::new("kill")
            .arg("-KILL")
            .arg(pid.to_string())
            .status();
    }
}
