//! Hot reload of `txtodo.toml` (task `workspace-layout`): applied when nothing sits in the old
//! place, refused while ref dirs do, and a bad or deleted file keeps the last good layout.

use crate::clock::FakeClock;
use crate::layout_file::LAYOUT_FILE;
use crate::layout_reload::{is_layout_file, reload_layout};
use crate::server::SharedWorkspace;
use crate::workspace::Workspace;
use std::path::Path;
use std::sync::{Arc, RwLock};

/// A workspace with one root-list line whose ref dir already sits in `tasks/` (the default).
fn open_with_a_ref_dir(root: &Path) -> SharedWorkspace {
    std::fs::write(root.join("todo.txt"), "(A) plan the launch ref:plan\n").unwrap();
    std::fs::create_dir_all(root.join("tasks/plan")).unwrap();
    std::fs::write(root.join("tasks/plan/todo.txt"), "draft the outline\n").unwrap();
    let ws = Workspace::open(root, Arc::new(FakeClock::new(1_000))).unwrap();
    Arc::new(RwLock::new(ws))
}

fn refs_dir(ws: &SharedWorkspace) -> String {
    ws.read().unwrap().layout().get().refs_dir().to_owned()
}

fn note(ws: &SharedWorkspace) -> Option<String> {
    ws.read().unwrap().layout().note()
}

#[tokio::test]
async fn a_workspace_with_no_layout_file_starts_on_tasks() {
    let dir = tempfile::tempdir().unwrap();
    let ws = open_with_a_ref_dir(dir.path());
    assert_eq!(refs_dir(&ws), "tasks");
    assert_eq!(note(&ws), None);
}

#[tokio::test]
async fn a_layout_file_read_at_open_wins_over_the_default() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join(LAYOUT_FILE), "refs_dir = \".\"\n").unwrap();
    let ws = open_with_a_ref_dir(dir.path());
    assert_eq!(refs_dir(&ws), ".");
}

#[tokio::test]
async fn a_change_is_refused_while_a_ref_dir_sits_in_the_old_place() {
    let dir = tempfile::tempdir().unwrap();
    let ws = open_with_a_ref_dir(dir.path());
    std::fs::write(dir.path().join(LAYOUT_FILE), "refs_dir = \"elsewhere\"\n").unwrap();

    reload_layout(&ws).await;

    assert_eq!(refs_dir(&ws), "tasks", "the layout in force did not move");
    assert!(note(&ws).unwrap().contains("refused"), "{:?}", note(&ws));
    assert!(
        dir.path().join("tasks/plan").is_dir(),
        "nothing was touched on disk"
    );
}

#[tokio::test]
async fn once_the_dirs_are_moved_the_same_file_applies() {
    let dir = tempfile::tempdir().unwrap();
    let ws = open_with_a_ref_dir(dir.path());
    std::fs::write(dir.path().join(LAYOUT_FILE), "refs_dir = \"elsewhere\"\n").unwrap();
    reload_layout(&ws).await;
    assert_eq!(refs_dir(&ws), "tasks");

    std::fs::create_dir_all(dir.path().join("elsewhere")).unwrap();
    std::fs::rename(
        dir.path().join("tasks/plan"),
        dir.path().join("elsewhere/plan"),
    )
    .unwrap();
    reload_layout(&ws).await;

    assert_eq!(refs_dir(&ws), "elsewhere");
    assert_eq!(
        note(&ws),
        None,
        "the note clears once the layout is in force"
    );
    assert!(
        ws.read().unwrap().tree_dirty.is_dirty(),
        "the tree is rebuilt against it"
    );
}

#[tokio::test]
async fn a_bad_or_deleted_file_keeps_the_last_good_layout() {
    let dir = tempfile::tempdir().unwrap();
    let ws = open_with_a_ref_dir(dir.path());
    std::fs::write(dir.path().join(LAYOUT_FILE), "refs_dir = [").unwrap();
    reload_layout(&ws).await;
    assert_eq!(refs_dir(&ws), "tasks");
    assert!(note(&ws).unwrap().contains("last good layout"));

    std::fs::remove_file(dir.path().join(LAYOUT_FILE)).unwrap();
    reload_layout(&ws).await;
    assert_eq!(
        refs_dir(&ws),
        "tasks",
        "deleting the file does not change what is in force"
    );
    assert_eq!(
        note(&ws),
        None,
        "the default needs no warning: a restart agrees"
    );
}

/// Task layout-reload-safety: a deleted file whose layout was not the default leaves a note,
/// since `layout_file::initial` falls back to the defaults on the next start.
#[tokio::test]
async fn deleting_the_file_under_a_non_default_layout_warns_about_the_next_start() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join(LAYOUT_FILE), "refs_dir = \".\"\n").unwrap();
    let ws = open_with_a_ref_dir(dir.path());
    assert_eq!(refs_dir(&ws), ".");

    std::fs::remove_file(dir.path().join(LAYOUT_FILE)).unwrap();
    reload_layout(&ws).await;
    assert_eq!(refs_dir(&ws), ".", "still in force for this run");
    let why = note(&ws).unwrap_or_default();
    assert!(
        why.contains("deleted") && why.contains("next start"),
        "{why}"
    );
}

/// Task layout-reload-safety: a `todo_file` that cannot be created (its name is taken by a
/// directory) refuses the RPC change before anything is written or switched.
#[test]
fn a_root_list_that_cannot_be_made_is_an_error_not_a_shrug() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("work.txt")).unwrap();
    let layout = txtodo_model::WorkspaceLayout::new("", "work.txt").unwrap();
    let err = crate::layout_reload::create_root_list_file(dir.path(), &layout).unwrap_err();
    assert!(err.to_string().contains("work.txt"), "{err}");
    let fine = txtodo_model::WorkspaceLayout::new("", "lists/work.txt").unwrap();
    crate::layout_reload::create_root_list_file(dir.path(), &fine).unwrap();
    assert!(dir.path().join("lists/work.txt").is_file());
}

#[test]
fn only_the_root_level_file_is_the_layout_file() {
    let root = Path::new("/w");
    assert!(is_layout_file(root, Path::new("/w/txtodo.toml")));
    assert!(!is_layout_file(
        root,
        Path::new("/w/tasks/plan/txtodo.toml")
    ));
    assert!(!is_layout_file(root, Path::new("/w/todo.txt")));
}
