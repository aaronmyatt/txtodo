//! Integration test (task `daemon-always-available`, item 3): with no `txtodod` running for a
//! fresh workspace, `app.rs::async_main`'s new ensure-daemon-then-wait sequence spawns one and
//! becomes ready, instead of the pre-task behavior of erroring on the very first connect. We can't
//! call `async_main` directly (it reads `std::env::current_dir()` and takes over the terminal), so
//! this replicates its handful of new lines: build a `LaunchConfig::new(&sock).with_dir(workspace)`
//! pointed at the real `txtodod` binary (never `$PATH`, so this test never depends on a system
//! install), call `ensure_daemon`, then `Daemon::connect` + `wait_until_ready` exactly as
//! `async_main` does.
//!
//! Unix-only (ADR 0010): a real `txtodod` means a real unix socket, same reasoning
//! `src/daemon.rs`'s own unit tests and this crate's other real-daemon integration tests are
//! gated for.
#![cfg(unix)]

mod support;

use std::time::Duration;

use txtodo_tui::daemon::{Daemon, socket_path};

/// No daemon spawned by this test (unlike `support::RealDaemon::start`, which spawns one up
/// front): a bare temp workspace with a `todo.txt` and nothing listening on its socket yet.
fn temp_workspace_no_daemon() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
    std::fs::write(dir.path().join("todo.txt"), "buy milk\n")
        .unwrap_or_else(|e| panic!("write todo.txt: {e}"));
    dir
}

/// Reads the pid `PidFile::acquire` wrote at `<workspace>/.txtodo/txtodod.pid` (plain decimal
/// text — `crates/txtodo-daemon/src/pidfile.rs`), for test cleanup only.
fn read_daemon_pid(workspace: &std::path::Path) -> Option<u32> {
    std::fs::read_to_string(workspace.join(".txtodo").join("txtodod.pid"))
        .ok()
        .and_then(|s| s.trim().parse().ok())
}

/// Best-effort SIGKILL by pid — same convention as `apps/desktop/src-tauri/tests/support::kill`.
/// A plain pid-file round trip, not a held `Child`: `ensure_daemon` spawns the process internally
/// and does not hand the child back to its caller.
fn kill(pid: u32) {
    let _ = std::process::Command::new("kill")
        .arg("-KILL")
        .arg(pid.to_string())
        .status();
}

#[tokio::test]
async fn connect_path_spawns_a_missing_daemon_and_becomes_ready() {
    let workspace = temp_workspace_no_daemon();
    let sock = socket_path(workspace.path());
    assert!(!sock.exists(), "no daemon should be listening yet");

    // The same few lines `app.rs::async_main` now runs before `wait_until_ready`, except
    // `daemon_bin` is pinned to the just-built test binary instead of `None` (which would search
    // `$PATH` — this test must never depend on a system install).
    let mut cfg = txtodo_daemon_launch::LaunchConfig::new(&sock).with_dir(workspace.path());
    cfg.daemon_bin = Some(support::daemon_bin().to_path_buf());
    txtodo_daemon_launch::ensure_daemon(&cfg)
        .await
        .unwrap_or_else(|e| panic!("ensure_daemon: {e}"));

    let mut daemon = Daemon::connect(&sock)
        .await
        .unwrap_or_else(|e| panic!("connect: {e}"));
    tokio::time::timeout(Duration::from_secs(10), daemon.wait_until_ready())
        .await
        .unwrap_or_else(|_| panic!("wait_until_ready timed out"))
        .unwrap_or_else(|e| {
            panic!("daemon never became ready after ensure_daemon spawned it: {e}")
        });

    if let Some(pid) = read_daemon_pid(workspace.path()) {
        kill(pid);
    }
}
