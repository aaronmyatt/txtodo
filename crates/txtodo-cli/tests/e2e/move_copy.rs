//! Task add-the-ability-to-move-or-copy-a-task-t: `mv` and `cp` against a real `txtodod` in true
//! global mode (hermetic via `$TXTODO_SOCKET`). `mv` used to fail here, since daemon mode ran it on
//! a scratch copy holding the root list alone; it now sends the daemon's cross-file Move, which
//! takes the task's `ref:` folder along. `cp` adds a new task without `id:` or `ref:`, dated today.
// Integration tests are tests: clippy.toml allows unwrap/expect in #[test] fns but not in their helpers.
#![allow(clippy::expect_used, clippy::unwrap_used)]

use crate::support;

use std::path::Path;
use std::process::Output;
use std::time::{Duration, Instant};
use support::global_daemon::GlobalDaemon;

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

fn read(root: &Path, rel: &str) -> String {
    std::fs::read_to_string(root.join(rel)).unwrap_or_default()
}

/// A workspace with two lists: the root one, whose line 2 has a ref folder with notes, and
/// `tasks/inbox/todo.txt`.
fn seed(daemon: &GlobalDaemon, root: &Path) {
    std::fs::write(root.join("todo.txt"), "call mum\nplan trip ref:trip\n").unwrap();
    std::fs::create_dir_all(root.join("tasks/trip")).unwrap();
    std::fs::write(root.join("tasks/trip/notes.md"), "the plan\n").unwrap();
    std::fs::create_dir_all(root.join("tasks/inbox")).unwrap();
    std::fs::write(root.join("tasks/inbox/todo.txt"), "sort mail\n").unwrap();
    let add = daemon.txtodo(root, &["workspace", "add", &root.display().to_string()]);
    assert!(add.status.success(), "{}", stderr(&add));
    // The daemon adopts the sub-list on its first walk; wait until `ls` through it answers.
    let deadline = Instant::now() + Duration::from_secs(20);
    while !stdout(&daemon.txtodo(root, &["--dir", "tasks/inbox", "ls"])).contains("sort mail") {
        assert!(
            Instant::now() < deadline,
            "the daemon never listed tasks/inbox"
        );
        std::thread::sleep(Duration::from_millis(200));
    }
}

#[test]
fn mv_moves_a_task_and_its_ref_folder_to_another_list() {
    let state = tempfile::tempdir().unwrap();
    let daemon = GlobalDaemon::spawn(state.path());
    let ws = tempfile::tempdir().unwrap();
    let root = ws.path();
    seed(&daemon, root);

    let moved = daemon.txtodo(root, &["mv", "2", "tasks/inbox/todo.txt"]);
    assert!(moved.status.success(), "{}", stderr(&moved));
    assert!(stdout(&moved).contains("2 plan trip ref:trip\nTODO: 2 moved to 2 in TODO."));
    assert!(
        !read(root, "todo.txt").contains("plan trip"),
        "{}",
        read(root, "todo.txt")
    );
    let inbox = read(root, "tasks/inbox/todo.txt");
    assert!(inbox.contains("plan trip ref:trip"), "{inbox}");
    assert_eq!(
        read(root, "tasks/inbox/trip/notes.md"),
        "the plan\n",
        "notes came along"
    );
    assert!(
        !root.join("tasks/trip").exists(),
        "the old ref folder is gone"
    );

    let nowhere = daemon.txtodo(root, &["mv", "1", "tasks/none/todo.txt"]);
    assert!(!nowhere.status.success());
    assert!(
        stderr(&nowhere).contains("Destination file"),
        "{}",
        stderr(&nowhere)
    );
}

#[test]
fn cp_adds_a_copy_without_its_ref_and_leaves_the_original() {
    let state = tempfile::tempdir().unwrap();
    let daemon = GlobalDaemon::spawn(state.path());
    let ws = tempfile::tempdir().unwrap();
    let root = ws.path();
    seed(&daemon, root);

    let copied = daemon.txtodo(root, &["cp", "2", "tasks/inbox/todo.txt"]);
    assert!(copied.status.success(), "{}", stderr(&copied));
    assert!(
        stdout(&copied).contains("TODO: 2 copied to 2 in TODO."),
        "{}",
        stdout(&copied)
    );
    assert!(
        read(root, "todo.txt").contains("plan trip ref:trip"),
        "the original stays"
    );
    let inbox = read(root, "tasks/inbox/todo.txt");
    let copy = inbox.lines().find(|l| l.ends_with(" plan trip"));
    assert!(copy.is_some(), "a new, dated task without ref:\n{inbox}");
    assert_eq!(read(root, "tasks/trip/notes.md"), "the plan\n");
}
