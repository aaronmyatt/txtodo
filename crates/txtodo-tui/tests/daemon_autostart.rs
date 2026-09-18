//! Integration test (task `daemon-always-available`, item 3; updated by task
//! `tui-global-socket-migration` for the switch to the device-global daemon): with no `txtodod`
//! running for a fresh workspace, `app.rs::async_main`'s ensure-daemon-then-wait sequence spawns
//! one and becomes ready, instead of the pre-task behavior of erroring on the very first connect.
//! We can't call `async_main` directly (it reads `std::env::current_dir()` and takes over the
//! terminal), so this replicates its handful of new lines: build a `LaunchConfig::new(&sock)`
//! (global-daemon shape, no `.with_dir`) pointed at the real `txtodod` binary (never `$PATH`, so
//! this test never depends on a system install) and at a hermetic temp socket/registry
//! (`TXTODO_SOCKET`/`TXTODO_REGISTRY_DB` in `extra_env` — same pattern
//! `apps/desktop/src-tauri/src/daemon/spawn.rs` uses for its own hermetic global-daemon tests, so
//! this test never touches the real machine's `$XDG_DATA_HOME/txtodo/`), call `ensure_daemon`,
//! then `Daemon::connect` with a `Path` selector + `wait_until_ready`, exactly as `async_main`
//! does.
//!
//! Unix-only (ADR 0010): a real `txtodod` means a real unix socket, same reasoning
//! `src/daemon.rs`'s own unit tests and this crate's other real-daemon integration tests are
//! gated for.
#![cfg(unix)]

mod support;

use std::time::Duration;

use txtodo_tui::daemon::{Daemon, workspace_selector};

/// No daemon spawned by this test (unlike `support::RealDaemon::start`, which spawns one up
/// front): a bare temp workspace with a `todo.txt` and nothing listening on its socket yet.
fn temp_workspace_no_daemon() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
    std::fs::write(dir.path().join("todo.txt"), "buy milk\n")
        .unwrap_or_else(|e| panic!("write todo.txt: {e}"));
    dir
}

/// Reads the pid `PidFile::acquire` wrote at `<state_dir>/txtodod.pid` (plain decimal text —
/// `crates/txtodo-daemon/src/pidfile.rs`), for test cleanup only. `state_dir` is this test's
/// hermetic temp dir (the parent of its `TXTODO_SOCKET` override), not the workspace itself —
/// global mode's pid lock lives beside the socket, not under `<workspace>/.txtodo/`.
fn read_daemon_pid(state_dir: &std::path::Path) -> Option<u32> {
    std::fs::read_to_string(state_dir.join("txtodod.pid"))
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
    let state_dir = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
    let sock = state_dir.path().join("txtodod.sock");
    let registry = state_dir.path().join("registry.db");
    assert!(!sock.exists(), "no daemon should be listening yet");

    // The same few lines `app.rs::async_main` now runs before `wait_until_ready`, except
    // `daemon_bin` is pinned to the just-built test binary instead of `None` (which would search
    // `$PATH` — this test must never depend on a system install), and `TXTODO_SOCKET`/
    // `TXTODO_REGISTRY_DB` are overridden so this hermetic global daemon never touches the real
    // machine's default location.
    let mut cfg = txtodo_daemon_launch::LaunchConfig::new(&sock);
    cfg.daemon_bin = Some(support::daemon_bin().to_path_buf());
    cfg.extra_env
        .push(("TXTODO_SOCKET".to_owned(), sock.display().to_string()));
    cfg.extra_env.push((
        "TXTODO_REGISTRY_DB".to_owned(),
        registry.display().to_string(),
    ));
    txtodo_daemon_launch::ensure_daemon(&cfg)
        .await
        .unwrap_or_else(|e| panic!("ensure_daemon: {e}"));

    let selector = workspace_selector(workspace.path());
    let mut daemon = Daemon::connect(&sock, Some(selector))
        .await
        .unwrap_or_else(|e| panic!("connect: {e}"));
    tokio::time::timeout(Duration::from_secs(10), daemon.wait_until_ready())
        .await
        .unwrap_or_else(|_| panic!("wait_until_ready timed out"))
        .unwrap_or_else(|e| {
            panic!("daemon never became ready after ensure_daemon spawned it: {e}")
        });

    if let Some(pid) = read_daemon_pid(state_dir.path()) {
        kill(pid);
    }
}

/// Acceptance (`tasks/tui-global-socket-migration/notes.md`): opening the TUI for a workspace
/// already served by another client's global daemon reuses that same daemon — no second process.
/// Runs `async_main`'s sequence twice against one hermetic global socket, for two different
/// workspaces (proving the `Path` selector really does route each `Daemon` to its own workspace
/// on the shared daemon, not just that a second spawn is skipped), and asserts the pid file names
/// the identical process both times.
#[tokio::test]
async fn a_second_workspace_reuses_the_already_running_global_daemon() {
    let workspace_a = temp_workspace_no_daemon();
    let workspace_b = temp_workspace_no_daemon();
    let state_dir = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
    let sock = state_dir.path().join("txtodod.sock");
    let registry = state_dir.path().join("registry.db");

    let mut cfg = txtodo_daemon_launch::LaunchConfig::new(&sock);
    cfg.daemon_bin = Some(support::daemon_bin().to_path_buf());
    cfg.extra_env
        .push(("TXTODO_SOCKET".to_owned(), sock.display().to_string()));
    cfg.extra_env.push((
        "TXTODO_REGISTRY_DB".to_owned(),
        registry.display().to_string(),
    ));

    // First "TUI open": workspace A, no daemon running yet — spawns one.
    txtodo_daemon_launch::ensure_daemon(&cfg)
        .await
        .unwrap_or_else(|e| panic!("ensure_daemon (a): {e}"));
    let mut daemon_a = Daemon::connect(&sock, Some(workspace_selector(workspace_a.path())))
        .await
        .unwrap_or_else(|e| panic!("connect (a): {e}"));
    daemon_a
        .wait_until_ready()
        .await
        .unwrap_or_else(|e| panic!("wait_until_ready (a): {e}"));
    let pid_after_first_open =
        read_daemon_pid(state_dir.path()).unwrap_or_else(|| panic!("no pid file after first open"));

    // Second "TUI open": workspace B, same global socket — ensure_daemon must see it already
    // live and spawn nothing new; the Path selector must still route to B, not A.
    txtodo_daemon_launch::ensure_daemon(&cfg)
        .await
        .unwrap_or_else(|e| panic!("ensure_daemon (b): {e}"));
    let mut daemon_b = Daemon::connect(&sock, Some(workspace_selector(workspace_b.path())))
        .await
        .unwrap_or_else(|e| panic!("connect (b): {e}"));
    daemon_b
        .wait_until_ready()
        .await
        .unwrap_or_else(|e| panic!("wait_until_ready (b): {e}"));
    let pid_after_second_open = read_daemon_pid(state_dir.path())
        .unwrap_or_else(|| panic!("no pid file after second open"));

    assert_eq!(
        pid_after_first_open, pid_after_second_open,
        "a second ensure_daemon call must reuse the already-running daemon, not spawn another"
    );

    let file_a = daemon_a
        .get_file("todo.txt")
        .await
        .unwrap_or_else(|e| panic!("get_file (a): {e}"));
    let file_b = daemon_b
        .get_file("todo.txt")
        .await
        .unwrap_or_else(|e| panic!("get_file (b): {e}"));
    assert_eq!(String::from_utf8_lossy(&file_a.bytes), "buy milk\n");
    assert_eq!(
        String::from_utf8_lossy(&file_b.bytes),
        "buy milk\n",
        "workspace B's own selector must route to B's own file, not A's"
    );

    kill(pid_after_second_open);
}
