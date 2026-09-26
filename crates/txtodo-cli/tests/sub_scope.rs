//! `txtodo sub` works a line's sub-list through the workspace that holds it (sync-drift line 3):
//! the `ref:` folder is never registered as a workspace of its own, and registering a folder
//! inside a registered workspace is refused with that workspace named. A real `txtodod` in true
//! global mode, hermetic via `$TXTODO_SOCKET`.
// Integration tests are tests: clippy.toml allows unwrap/expect in #[test] fns but not in their helpers.
#![allow(clippy::expect_used, clippy::unwrap_used)]

mod support;

use std::path::Path;
use std::process::{Command, Output};
use std::time::{Duration, Instant};
use support::global_daemon::GlobalDaemon;

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

/// `txtodo notes 1` with a no-op `$EDITOR`: makes line 1's ref dir lazily, nothing else.
fn open_notes(daemon: &GlobalDaemon, ws: &Path) {
    let notes = Command::new(env!("CARGO_BIN_EXE_txtodo"))
        .current_dir(ws)
        .env_remove("TXTODO_TODO_DIR")
        .env("TXTODO_CONFIG", ws.join("none.toml"))
        .env("TXTODO_SOCKET", &daemon.socket)
        .env("TXTODO_NO_AUTOSTART", "1")
        .env("EDITOR", "true")
        .args(["notes", "1"])
        .output()
        .unwrap_or_else(|e| panic!("notes: {e}"));
    assert!(notes.status.success(), "{}", stderr(&notes));
}

/// `sub 1 ls` until it shows `want` or 20 s pass: a sub-list written straight to disk is adopted
/// by the daemon's watcher, so a listing can lag the write.
fn sub_ls_until(daemon: &GlobalDaemon, ws: &Path, want: &str) -> String {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let out = stdout(&daemon.txtodo(ws, &["sub", "1", "ls"]));
        if out.contains(want) || Instant::now() >= deadline {
            return out;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

#[test]
fn sub_works_the_sub_list_through_its_workspace_and_registers_no_other() {
    let state = tempfile::tempdir().unwrap();
    let daemon = GlobalDaemon::spawn(state.path());
    let ws = tempfile::tempdir().unwrap();
    std::fs::write(ws.path().join("todo.txt"), "plan trip\n").unwrap();
    open_notes(&daemon, ws.path());
    let ref_dir = ws.path().join("tasks/plan-trip");
    assert!(ref_dir.is_dir(), "notes made the ref dir");

    // Two adds back to back: the second may reach the daemon before its watcher has adopted the
    // new sub-list, and must still keep the first line.
    for line in ["book hotel", "pack bags"] {
        let add = daemon.txtodo(ws.path(), &["sub", "1", "add", line]);
        assert!(add.status.success(), "{}", stderr(&add));
    }
    let nested = std::fs::read_to_string(ref_dir.join("todo.txt")).unwrap();
    assert!(
        nested.contains("book hotel") && nested.contains("pack bags"),
        "{nested}"
    );
    let listed = sub_ls_until(&daemon, ws.path(), "pack bags");
    assert!(listed.contains("book hotel"), "{listed}");

    assert_only_the_workspace_is_registered(&daemon, ws.path(), &ref_dir);
}

/// The ref dir is no workspace: not listed, no daemon state in it, and adding it by hand is
/// refused with the workspace that holds it named.
fn assert_only_the_workspace_is_registered(daemon: &GlobalDaemon, ws: &Path, ref_dir: &Path) {
    let workspaces = daemon.txtodo(ws, &["workspace", "list"]);
    assert!(workspaces.status.success(), "{}", stderr(&workspaces));
    assert!(
        !stdout(&workspaces).contains("plan-trip"),
        "{}",
        stdout(&workspaces)
    );
    assert!(
        !ref_dir.join(".txtodo").exists(),
        "no daemon state in the ref dir"
    );
    let dir = ref_dir.display().to_string();
    let refused = daemon.txtodo(ws, &["workspace", "add", &dir]);
    assert!(!refused.status.success(), "{}", stdout(&refused));
    let root = ws
        .canonicalize()
        .unwrap_or_else(|e| panic!("canonicalize: {e}"));
    let why = stderr(&refused);
    assert!(why.contains("inside the registered workspace"), "{why}");
    assert!(why.contains(&root.display().to_string()), "{why}");
}
