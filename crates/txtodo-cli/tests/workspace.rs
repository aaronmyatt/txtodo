//! `txtodo workspace add|remove|list` and the CLI's global-socket fallback end to end (ADR 0025,
//! task `cli-workspace-commands`): a real `txtodod` in TRUE GLOBAL mode (`--dir` omitted), hermetic
//! via `$TXTODO_SOCKET`, with no per-directory socket anywhere — so every call here only succeeds
//! if `client::select` actually falls through to the global daemon and attaches a real `Path`
//! selector, not the pre-existing `--dir`-bridge path `daemon_mode.rs` already covers.
// Integration tests are tests: clippy.toml allows unwrap/expect in #[test] fns but not in their helpers.
#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

const SOCKET_WAIT: Duration = Duration::from_secs(20);

fn txtodod_binary() -> PathBuf {
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
    bin
}

/// A real `txtodod` in true global mode (`--dir` omitted), hermetic via `$TXTODO_SOCKET`/
/// `$TXTODO_REGISTRY_DB` so it never touches this machine's real `$XDG_DATA_HOME/txtodo/`.
struct GlobalDaemon {
    child: Child,
    socket: PathBuf,
}

impl GlobalDaemon {
    fn spawn(state_dir: &Path) -> GlobalDaemon {
        let socket = state_dir.join("txtodod.sock");
        let child = Command::new(txtodod_binary())
            .env("TXTODO_SOCKET", &socket)
            .env("TXTODO_REGISTRY_DB", state_dir.join("registry.db"))
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap_or_else(|e| panic!("spawn txtodod: {e}"));
        let start = Instant::now();
        while !socket.exists() {
            assert!(
                start.elapsed() < SOCKET_WAIT,
                "daemon socket did not appear"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
        GlobalDaemon { child, socket }
    }
}

impl Drop for GlobalDaemon {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Runs `txtodo` against `daemon`'s socket, from `dir` — which must have no `.txtodo/txtodod.sock`
/// of its own, so `client::select` has nothing to fall back to except the global daemon.
fn txtodo(daemon: &GlobalDaemon, dir: &Path, args: &[&str]) -> Output {
    assert!(
        !dir.join(".txtodo").join("txtodod.sock").exists(),
        "this harness only proves the global-daemon path"
    );
    Command::new(env!("CARGO_BIN_EXE_txtodo"))
        .current_dir(dir)
        .env_remove("TXTODO_TODO_DIR")
        .env("TXTODO_CONFIG", dir.join("none.toml"))
        .env("TXTODO_SOCKET", &daemon.socket)
        .args(args)
        .output()
        .unwrap_or_else(|e| panic!("txtodo: {e}"))
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

#[test]
fn add_remove_list_round_trip_and_add_is_idempotent() {
    let state_dir = tempfile::tempdir().unwrap();
    let daemon = GlobalDaemon::spawn(state_dir.path());
    let ws_dir = tempfile::tempdir().unwrap();
    std::fs::write(ws_dir.path().join("todo.txt"), "").unwrap();

    let empty = txtodo(&daemon, ws_dir.path(), &["workspace", "list"]);
    assert!(empty.status.success(), "{}", stdout(&empty));
    assert!(
        stdout(&empty).contains("no registered workspaces"),
        "{}",
        stdout(&empty)
    );

    let add = txtodo(&daemon, ws_dir.path(), &["workspace", "add"]);
    assert!(
        add.status.success(),
        "{}",
        String::from_utf8_lossy(&add.stderr)
    );
    let added = stdout(&add);
    let id = added
        .split_whitespace()
        .next()
        .unwrap_or_else(|| panic!("no id in: {added}"))
        .to_owned();

    // Idempotent: adding the same directory again returns the same id, not a duplicate entry.
    let add_again = txtodo(&daemon, ws_dir.path(), &["workspace", "add"]);
    assert!(
        stdout(&add_again).starts_with(&id),
        "{}",
        stdout(&add_again)
    );
    let list = txtodo(&daemon, ws_dir.path(), &["workspace", "list"]);
    assert_eq!(
        stdout(&list).lines().count(),
        1,
        "still one entry: {}",
        stdout(&list)
    );

    let remove = txtodo(&daemon, ws_dir.path(), &["workspace", "remove", &id]);
    assert!(remove.status.success(), "{}", stdout(&remove));
    let after = txtodo(&daemon, ws_dir.path(), &["workspace", "list"]);
    assert!(
        stdout(&after).contains("no registered workspaces"),
        "{}",
        stdout(&after)
    );
}

#[test]
fn a_todo_command_with_no_per_dir_socket_reaches_the_global_daemon() {
    let state_dir = tempfile::tempdir().unwrap();
    let daemon = GlobalDaemon::spawn(state_dir.path());
    let ws_dir = tempfile::tempdir().unwrap();
    std::fs::write(ws_dir.path().join("todo.txt"), "").unwrap();

    // No `workspace add` first: the Path selector auto-registers this directory, the same bridge
    // `WorkspaceCatalog::resolve` already proves at the daemon level (workspace_catalog_tests.rs).
    let add = txtodo(&daemon, ws_dir.path(), &["add", "buy", "milk"]);
    assert!(
        add.status.success(),
        "{}",
        String::from_utf8_lossy(&add.stderr)
    );
    assert!(stdout(&add).contains("TODO: 1 added."), "{}", stdout(&add));

    let log = txtodo(&daemon, ws_dir.path(), &["log"]);
    assert!(log.status.success(), "{}", stdout(&log));
    assert!(
        stdout(&log).contains("you@"),
        "the add reached the daemon, not direct-file mode: {}",
        stdout(&log)
    );

    let list = txtodo(&daemon, ws_dir.path(), &["workspace", "list"]);
    assert_eq!(
        stdout(&list).lines().count(),
        1,
        "auto-registered exactly once: {}",
        stdout(&list)
    );
}
