//! `txtodo open` / `sub` follow the workspace layout end to end (task `workspace-layout`): with the
//! default layout a root line's `ref:` directory is `tasks/<slug>`, both when it already exists and
//! when the CLI creates it. A real `txtodod` in true global mode, hermetic via `$TXTODO_SOCKET`.
// Integration tests are tests: clippy.toml allows unwrap/expect in #[test] fns but not in their helpers.
#![allow(clippy::expect_used, clippy::unwrap_used)]

mod support;

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};
use support::txtodod_binary;

const SOCKET_WAIT: Duration = Duration::from_secs(20);

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

fn workspace(todo: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("todo.txt"), todo).unwrap();
    dir
}

#[test]
fn open_and_sub_resolve_an_existing_ref_dir_in_tasks() {
    let state = tempfile::tempdir().unwrap();
    let daemon = GlobalDaemon::spawn(state.path());
    let ws = workspace("(A) plan the launch ref:plan\n");
    std::fs::create_dir_all(ws.path().join("tasks/plan")).unwrap();
    std::fs::write(ws.path().join("tasks/plan/todo.txt"), "draft the outline\n").unwrap();

    let open = txtodo(&daemon, ws.path(), &["open", "1"]);
    assert!(
        open.status.success(),
        "{}",
        String::from_utf8_lossy(&open.stderr)
    );
    let printed = stdout(&open);
    assert!(printed.trim_end().ends_with("tasks/plan"), "{printed}");

    let sub = txtodo(&daemon, ws.path(), &["sub", "1", "add", "second step"]);
    assert!(
        sub.status.success(),
        "{}",
        String::from_utf8_lossy(&sub.stderr)
    );
    let nested = std::fs::read_to_string(ws.path().join("tasks/plan/todo.txt")).unwrap();
    assert!(nested.contains("second step"), "{nested}");
    assert!(!ws.path().join("plan").exists(), "nothing beside the list");
}

#[test]
fn sub_on_a_line_with_no_ref_creates_it_in_tasks() {
    let state = tempfile::tempdir().unwrap();
    let daemon = GlobalDaemon::spawn(state.path());
    let ws = workspace("buy ducks\n");

    // `notes` makes the ref dir lazily and opens $EDITOR on its notes.md; `true` is a no-op editor.
    let notes = Command::new(env!("CARGO_BIN_EXE_txtodo"))
        .current_dir(ws.path())
        .env_remove("TXTODO_TODO_DIR")
        .env("TXTODO_CONFIG", ws.path().join("none.toml"))
        .env("TXTODO_SOCKET", &daemon.socket)
        .env("EDITOR", "true")
        .args(["notes", "1"])
        .output()
        .unwrap();
    assert!(
        notes.status.success(),
        "{}",
        String::from_utf8_lossy(&notes.stderr)
    );
    assert!(
        ws.path().join("tasks/buy-ducks").is_dir(),
        "the dir is made in tasks/"
    );

    let sub = txtodo(&daemon, ws.path(), &["sub", "1", "add", "pond first"]);
    assert!(
        sub.status.success(),
        "{}",
        String::from_utf8_lossy(&sub.stderr)
    );

    let root = std::fs::read_to_string(ws.path().join("todo.txt")).unwrap();
    assert!(root.contains("ref:buy-ducks"), "{root}");
    let nested = std::fs::read_to_string(ws.path().join("tasks/buy-ducks/todo.txt")).unwrap();
    assert!(nested.contains("pond first"), "{nested}");
    assert!(
        !ws.path().join("buy-ducks").exists(),
        "nothing beside the list"
    );
}

fn with_a_live_ref_dir() -> (tempfile::TempDir, tempfile::TempDir, GlobalDaemon) {
    let state = tempfile::tempdir().unwrap();
    let daemon = GlobalDaemon::spawn(state.path());
    let ws = workspace("(A) plan the launch ref:plan\n");
    std::fs::create_dir_all(ws.path().join("tasks/plan")).unwrap();
    std::fs::write(ws.path().join("tasks/plan/todo.txt"), "draft\n").unwrap();
    (state, ws, daemon)
}

#[test]
fn workspace_layout_shows_the_layout_and_refuses_a_change_under_a_live_ref_dir() {
    let (_state, ws, daemon) = with_a_live_ref_dir();
    let shown = stdout(&txtodo(&daemon, ws.path(), &["workspace", "layout"]));
    assert!(
        shown.contains("refs_dir  = tasks") && shown.contains("todo_file = todo.txt"),
        "{shown}"
    );

    let refused = txtodo(
        &daemon,
        ws.path(),
        &["workspace", "layout", "--refs-dir", "stuff"],
    );
    assert!(!refused.status.success());
    let why = String::from_utf8_lossy(&refused.stderr).into_owned();
    assert!(why.contains("ref dir"), "{why}");
    assert!(!ws.path().join("txtodo.toml").exists());
}

#[test]
fn workspace_layout_move_relocates_the_dirs_and_open_and_doctor_follow() {
    let (_state, ws, daemon) = with_a_live_ref_dir();
    let args = ["workspace", "layout", "--refs-dir", "stuff", "--move"];
    let moved = stdout(&txtodo(&daemon, ws.path(), &args));
    assert!(
        moved.contains("refs_dir  = stuff") && moved.contains("moved 1"),
        "{moved}"
    );
    assert!(ws.path().join("stuff/plan/todo.txt").is_file());
    assert!(!ws.path().join("tasks/plan").exists());
    assert!(
        std::fs::read_to_string(ws.path().join("txtodo.toml"))
            .unwrap()
            .contains("stuff")
    );
    assert!(
        stdout(&txtodo(&daemon, ws.path(), &["open", "1"]))
            .trim_end()
            .ends_with("stuff/plan")
    );

    let doctor = stdout(&txtodo(&daemon, ws.path(), &["doctor"]));
    let row = doctor
        .lines()
        .find(|l| l.starts_with("layout"))
        .unwrap_or_else(|| panic!("{doctor}"));
    assert!(row.contains("refs_dir = stuff"), "{row}");
}
