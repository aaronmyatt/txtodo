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

/// todo `ref:cli-workspace-autoregister`: a completely fresh directory — no `todo.txt`, no
/// `.txtodo/`, nothing — with a global daemon reachable and no separate `workspace add` step.
/// `daemon_mode::run_via_daemon`'s very first call (`list_files`) already carries the same Path
/// selector every other RPC does (`client.rs::select`), so this needs no new production code —
/// only this regression test, guarding the behavior `cli-workspace-commands` already delivered.
#[test]
fn first_add_in_a_brand_new_directory_auto_registers_it_with_no_separate_step() {
    let state_dir = tempfile::tempdir().unwrap();
    let daemon = GlobalDaemon::spawn(state_dir.path());
    let ws_dir = tempfile::tempdir().unwrap();
    assert!(!ws_dir.path().join("todo.txt").exists());

    let add = txtodo(&daemon, ws_dir.path(), &["add", "first", "task"]);
    assert!(
        add.status.success(),
        "{}",
        String::from_utf8_lossy(&add.stderr)
    );
    assert!(stdout(&add).contains("TODO: 1 added."), "{}", stdout(&add));
    assert!(
        ws_dir.path().join("todo.txt").exists(),
        "add creates a missing todo.txt, same as todo.sh"
    );

    let list = txtodo(&daemon, ws_dir.path(), &["workspace", "list"]);
    let out = stdout(&list);
    assert_eq!(
        out.lines().count(),
        1,
        "auto-registered, no manual step: {out}"
    );
    assert!(out.contains(&ws_dir.path().display().to_string()), "{out}");
}

/// todo `ref:cli-doctor-multi-workspace`: `txtodo doctor` from one workspace reports every *other*
/// registered workspace too, not just the cwd's — one `workspace` row per entry, alongside the
/// seven fixed checks the cwd's own workspace already gets in full depth.
#[test]
fn doctor_reports_every_other_registered_workspace() {
    let state_dir = tempfile::tempdir().unwrap();
    let daemon = GlobalDaemon::spawn(state_dir.path());
    let dir_a = tempfile::tempdir().unwrap();
    let dir_b = tempfile::tempdir().unwrap();
    std::fs::write(dir_a.path().join("todo.txt"), "").unwrap();
    std::fs::write(dir_b.path().join("todo.txt"), "").unwrap();

    assert!(
        txtodo(&daemon, dir_a.path(), &["add", "in a"])
            .status
            .success()
    );
    assert!(
        txtodo(&daemon, dir_b.path(), &["add", "in b"])
            .status
            .success()
    );

    let doctor_a = txtodo(&daemon, dir_a.path(), &["doctor"]);
    assert!(
        doctor_a.status.success(),
        "{}",
        String::from_utf8_lossy(&doctor_a.stderr)
    );
    let out = stdout(&doctor_a);
    let workspace_rows: Vec<&str> = out.lines().filter(|l| l.starts_with("workspace")).collect();
    assert_eq!(
        workspace_rows.len(),
        1,
        "exactly one other-workspace row (b, not a): {out}"
    );
    assert!(
        workspace_rows[0].contains(&dir_b.path().display().to_string()),
        "the row names b, not a: {workspace_rows:?}"
    );
}
