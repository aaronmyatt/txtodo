//! Root todo id:01M2VV1ZXD42Z8QP85SHVQSDXT: a second `txtodod` against a live one must lose the pid
//! lock *before* it opens anything. Until 2026-09-20 `main.rs::run` took the lock after
//! `start_global` had opened (and rebuilt the Loro mirror of) every registered workspace, so four
//! daemons on one device each paid the whole cold open before three learned they had lost — the
//! 300% CPU boot storm of 2026-09-19.
//!
//! Observable proof without timing: the loser's stderr never carries the `opened N registered
//! workspace(s)` line `start_global` prints once its open pass ends, and it names the running pid.
//! It also exits 0 (root todo id:01M2VV1ZXDK24H3P6Z4DJ2P8YM): there is nothing for launchd's
//! `KeepAlive.SuccessfulExit=false` or systemd's `Restart=on-failure` to retry.
// Integration tests are tests: clippy.toml allows unwrap/expect in #[test] fns but not in their helpers.
#![allow(clippy::expect_used, clippy::unwrap_used)]
// The daemon's only transport is a unix-domain socket (ADR 0010).
#![cfg(unix)]

use std::io::Read;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};
use txtodo_daemon::clock::SystemClock;
use txtodo_daemon::workspace_registry::WorkspaceRegistry;

/// Generous: a debug `txtodod` on a loaded CI runner. The loser exits in milliseconds; this is
/// only the bound that stops a regression from hanging the suite.
const EXIT_WAIT: Duration = Duration::from_secs(30);
const SOCKET_WAIT: Duration = Duration::from_secs(120);

/// Spawns a global-mode `txtodod` whose registry, socket, pid lock and logs all live in `dir`.
fn spawn_global(dir: &std::path::Path) -> Child {
    // https://doc.rust-lang.org/std/process/struct.Command.html#method.env
    Command::new(env!("CARGO_BIN_EXE_txtodod"))
        .args(["--no-lan", "--no-relay"])
        .env("TXTODO_REGISTRY_DB", dir.join("registry.db"))
        .env("TXTODO_SOCKET", dir.join("txtodod.sock"))
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn txtodod")
}

#[test]
fn a_second_daemon_exits_on_the_pid_lock_without_opening_any_workspace() {
    let state = tempfile::tempdir().expect("tempdir");
    let workspace = tempfile::tempdir().expect("tempdir");
    std::fs::write(workspace.path().join("todo.txt"), "seed\n").expect("seed todo.txt");
    WorkspaceRegistry::open(&state.path().join("registry.db"))
        .expect("open registry")
        .add(workspace.path(), &SystemClock)
        .expect("register workspace");

    let mut first = spawn_global(state.path());
    let socket = state.path().join("txtodod.sock");
    let start = Instant::now();
    while !socket.exists() {
        assert!(start.elapsed() < SOCKET_WAIT, "first daemon never bound");
        std::thread::sleep(Duration::from_millis(20));
    }

    let mut second = spawn_global(state.path());
    let start = Instant::now();
    let status = loop {
        if let Some(status) = second.try_wait().expect("poll second daemon") {
            break status;
        }
        assert!(
            start.elapsed() < EXIT_WAIT,
            "the second daemon should exit on the pid lock, but is still running"
        );
        std::thread::sleep(Duration::from_millis(10));
    };
    let mut stderr = String::new();
    second
        .stderr
        .take()
        .expect("piped stderr")
        .read_to_string(&mut stderr)
        .expect("read stderr");

    assert!(
        status.success(),
        "losing the lock is not a failure: {stderr}"
    );
    assert!(
        stderr.contains("already running"),
        "the loser should name the pid lock: {stderr}"
    );
    assert!(
        !stderr.contains("registered workspace"),
        "the loser opened workspaces before it lost the lock: {stderr}"
    );
    assert!(
        socket.exists(),
        "the loser must not remove the live daemon's socket"
    );
    assert!(
        first.try_wait().expect("poll first daemon").is_none(),
        "the first daemon must keep running"
    );

    let _ = first.kill();
    let _ = first.wait();
}
