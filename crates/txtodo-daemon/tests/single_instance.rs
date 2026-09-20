//! Root todo id:01M2VV1ZXD42Z8QP85SHVQSDXT: a second `txtodod` against a live one must lose the pid
//! lock *before* it opens anything. Until 2026-09-20 `main.rs::run` took the lock after
//! `start_global` had opened (and rebuilt the Loro mirror of) every registered workspace, so four
//! daemons on one device each paid the whole cold open before three learned they had lost — the
//! 300% CPU boot storm of 2026-09-19.
//!
//! Observable proof without timing: the loser is given a registry of its own, naming a workspace
//! only it knows, and that workspace's directory never gains a `.txtodo/` state folder (every open
//! creates one). It also names the running pid. An earlier version asserted that stderr lacked a
//! log line, which the same change had deleted, so it could not fail (code review 2026-09-20,
//! finding 10); and the winner opens the shared workspace itself, so that one proves nothing.
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

/// Spawns a global-mode `txtodod` whose socket, pid lock and logs live in `dir`, reading the
/// workspace registry at `registry`.
fn spawn_global(dir: &std::path::Path, registry: &std::path::Path) -> Child {
    // https://doc.rust-lang.org/std/process/struct.Command.html#method.env
    Command::new(env!("CARGO_BIN_EXE_txtodod"))
        .args(["--no-lan", "--no-relay"])
        .env("TXTODO_REGISTRY_DB", registry)
        .env("TXTODO_SOCKET", dir.join("txtodod.sock"))
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn txtodod")
}

/// A seeded workspace, registered in the registry database at `registry`.
fn registered_workspace(registry: &std::path::Path) -> tempfile::TempDir {
    let workspace = tempfile::tempdir().expect("tempdir");
    std::fs::write(workspace.path().join("todo.txt"), "seed\n").expect("seed todo.txt");
    WorkspaceRegistry::open(registry)
        .expect("open registry")
        .add(workspace.path(), &SystemClock)
        .expect("register workspace");
    workspace
}

/// Polls `done` until it holds, failing with `what` after `limit`.
fn wait_until(what: &str, limit: Duration, mut done: impl FnMut() -> bool) {
    let start = Instant::now();
    while !done() {
        assert!(start.elapsed() < limit, "timed out: {what}");
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// Waits for `child` to exit and returns its status and everything it wrote to stderr.
fn exit_of(mut child: Child) -> (std::process::ExitStatus, String) {
    let mut status = None;
    wait_until(
        "the second daemon to exit on the pid lock",
        EXIT_WAIT,
        || {
            status = child.try_wait().expect("poll second daemon");
            status.is_some()
        },
    );
    let mut stderr = String::new();
    child
        .stderr
        .take()
        .expect("piped stderr")
        .read_to_string(&mut stderr)
        .expect("read stderr");
    (status.expect("exited"), stderr)
}

#[test]
fn a_second_daemon_exits_on_the_pid_lock_without_opening_any_workspace() {
    let state = tempfile::tempdir().expect("tempdir");
    let registry = state.path().join("registry.db");
    let workspace = registered_workspace(&registry);
    // The loser's own registry and workspace: only the loser could ever open this one.
    let losers_registry = state.path().join("loser-registry.db");
    let losers_workspace = registered_workspace(&losers_registry);

    let mut first = spawn_global(state.path(), &registry);
    let socket = state.path().join("txtodod.sock");
    wait_until("the first daemon to bind", SOCKET_WAIT, || socket.exists());

    let (status, stderr) = exit_of(spawn_global(state.path(), &losers_registry));

    assert!(
        status.success(),
        "losing the lock is not a failure: {stderr}"
    );
    assert!(
        stderr.contains("already running"),
        "the loser should name the pid lock: {stderr}"
    );
    assert!(
        !losers_workspace.path().join(".txtodo").exists(),
        "the loser opened its workspace before it lost the lock: {stderr}"
    );
    // Positive control for the check above: an open does leave a `.txtodo/` folder. The winner
    // opens its registered workspace in the background, so that one gains it.
    wait_until("the winner to open its workspace", SOCKET_WAIT, || {
        workspace.path().join(".txtodo").exists()
    });
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
