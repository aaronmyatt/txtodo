//! Moving a copy aside for a rejoin (task sync-drift line 8), on real temp folders: what moves, in
//! what order, the guard, and that a failure moves everything back without overwriting anything.

use std::path::Path;

use super::{GUARD_HEAD, Plan, move_aside, move_back, plan, remove_guard, stamp};

/// 2026-09-26T15:02:11Z.
const NOW_MS: u64 = 1_790_434_931_000;

fn write(root: &Path, rel: &str, text: &str) {
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().unwrap_or_else(|| panic!("a parent")))
        .unwrap_or_else(|e| panic!("mkdir: {e}"));
    std::fs::write(&path, text).unwrap_or_else(|e| panic!("write {rel}: {e}"));
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

/// A workspace with state, a layout, three documents and two files that are not documents.
fn workspace() -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
    let root = dir.path().join("todo");
    write(&root, ".txtodo/oplog.db", "old store");
    write(&root, "txtodo.toml", "refs_dir = \"tasks\"\n");
    write(&root, "todo.txt", "(A) twice\n(A) twice\n");
    write(&root, "tasks/a/todo.txt", "sub line\n");
    write(&root, "tasks/a/notes.md", "a plan\n");
    write(&root, "README.md", "not a document\n");
    write(&root, "tasks/a/report.txt", "not a document either\n");
    (dir, root)
}

#[test]
fn the_stamp_is_utc_to_the_second_without_colons() {
    assert_eq!(stamp(0), "1970-01-01T000000Z");
    assert_eq!(stamp(NOW_MS), "2026-09-26T150211Z");
}

#[test]
fn a_plan_names_state_first_then_the_layout_then_each_document_and_makes_nothing() {
    let (_dir, root) = workspace();
    let p = plan(&root, NOW_MS).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(
        p.entries(),
        [
            ".txtodo",
            "txtodo.toml",
            "tasks/a/notes.md",
            "tasks/a/todo.txt",
            "todo.txt"
        ]
    );
    let parent = root.parent().unwrap_or_else(|| panic!("a parent"));
    assert_eq!(
        p.backup,
        parent.join("todo.rejoin-backup-2026-09-26T150211Z")
    );
    assert!(!p.backup.exists(), "a plan only reads");

    std::fs::create_dir(&p.backup).unwrap_or_else(|e| panic!("mkdir: {e}"));
    let next = plan(&root, NOW_MS).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(
        next.backup,
        parent.join("todo.rejoin-backup-2026-09-26T150211Z-2")
    );
}

/// Every document and the layout are in `backup`; files that are not documents stayed in `root`.
fn assert_documents_moved(root: &Path, backup: &Path) {
    assert_eq!(read(&backup.join("todo.txt")), "(A) twice\n(A) twice\n");
    assert_eq!(read(&backup.join("tasks/a/notes.md")), "a plan\n");
    assert!(backup.join("txtodo.toml").is_file());
    for gone in [
        "todo.txt",
        "txtodo.toml",
        "tasks/a/todo.txt",
        "tasks/a/notes.md",
    ] {
        assert!(!root.join(gone).exists(), "{gone} moved");
    }
    for stays in ["README.md", "tasks/a/report.txt"] {
        assert!(root.join(stays).is_file(), "{stays} is not a document");
    }
}

#[test]
fn moving_aside_takes_every_document_and_the_state_and_leaves_a_guard() {
    let (_dir, root) = workspace();
    let p = plan(&root, NOW_MS).unwrap_or_else(|e| panic!("{e}"));
    move_aside(&p).unwrap_or_else(|e| panic!("{e}"));

    assert_documents_moved(&root, &p.backup);
    assert_eq!(read(&p.backup.join(".txtodo/oplog.db")), "old store");
    assert!(read(&root.join(".txtodo")).starts_with(GUARD_HEAD));

    remove_guard(&root).unwrap_or_else(|e| panic!("{e}"));
    assert!(!root.join(".txtodo").exists(), "the guard is gone");
    assert!(crate::join_target::require_empty(&root, "rejoin", "").is_ok());
}

#[test]
fn a_failed_move_puts_everything_back_and_removes_the_backup() {
    let (_dir, root) = workspace();
    let mut p = plan(&root, NOW_MS).unwrap_or_else(|e| panic!("{e}"));
    // A document that vanished after the plan: its rename fails half way through.
    p.documents.insert(2, "tasks/gone/todo.txt".to_owned());

    let err = move_aside(&p)
        .err()
        .unwrap_or_else(|| panic!("the move must fail"));
    assert!(err.contains("tasks/gone/todo.txt"), "{err}");
    assert!(err.contains("nothing was moved in the end"), "{err}");
    assert_eq!(read(&root.join(".txtodo/oplog.db")), "old store");
    assert_eq!(read(&root.join("txtodo.toml")), "refs_dir = \"tasks\"\n");
    assert_eq!(read(&root.join("tasks/a/notes.md")), "a plan\n");
    assert!(!p.backup.exists(), "an empty backup is removed");
}

#[test]
fn moving_back_never_overwrites_and_names_what_stayed() {
    let (_dir, root) = workspace();
    let p = plan(&root, NOW_MS).unwrap_or_else(|e| panic!("{e}"));
    move_aside(&p).unwrap_or_else(|e| panic!("{e}"));
    write(&root, "todo.txt", "written meanwhile\n");

    let err = move_back(&p, &p.entries())
        .err()
        .unwrap_or_else(|| panic!("todo.txt stays"));
    assert!(err.contains("todo.txt (something new is there)"), "{err}");
    assert_eq!(read(&root.join("todo.txt")), "written meanwhile\n");
    assert_eq!(read(&p.backup.join("todo.txt")), "(A) twice\n(A) twice\n");
    assert_eq!(
        read(&root.join(".txtodo/oplog.db")),
        "old store",
        "state came back"
    );
    assert!(root.join(".txtodo").is_dir(), "the guard is gone");
}

#[test]
fn a_guard_left_by_a_crash_moves_like_state() {
    let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
    let root = dir.path().join("w");
    write(&root, ".txtodo", &format!("{GUARD_HEAD}: an earlier run\n"));
    write(&root, "todo.txt", "left behind\n");
    let p: Plan = plan(&root, NOW_MS).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(p.entries(), [".txtodo", "todo.txt"]);
    move_aside(&p).unwrap_or_else(|e| panic!("{e}"));
    assert!(read(&p.backup.join(".txtodo")).contains("an earlier run"));
    assert!(read(&root.join(".txtodo")).starts_with(GUARD_HEAD));
}
