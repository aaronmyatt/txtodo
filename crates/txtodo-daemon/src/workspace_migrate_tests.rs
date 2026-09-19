//! `Workspace::begin_sidecar_migration` + `migrate_documents` (tasks/sidecar-migrate-tagged): a
//! whole Tagged workspace converts, survives a reopen as Sidecar, and a half-done run resumes.

use crate::clock::FakeClock;
use crate::workspace::Workspace;
use crate::workspace_migrate::migrate_documents;
use std::path::Path;
use std::sync::Arc;
use txtodo_model::IdentityMode;

const A: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAA";
const B: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAB";
const C: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAC";

fn touch(p: &Path, bytes: &str) {
    if let Some(d) = p.parent() {
        std::fs::create_dir_all(d).unwrap_or_else(|e| panic!("{e}"));
    }
    std::fs::write(p, bytes).unwrap_or_else(|e| panic!("{e}"));
}

fn read(p: &Path) -> String {
    std::fs::read_to_string(p).unwrap_or_default()
}

fn open(root: &Path) -> Workspace {
    Workspace::open(root, Arc::new(FakeClock::new(1_000)))
        .unwrap_or_else(|e| panic!("open workspace: {e}"))
}

fn seed(root: &Path) {
    touch(&root.join("todo.txt"), &format!("one id:{A}\ntwo id:{B}\n"));
    touch(&root.join("q4/todo.txt"), &format!("three id:{C}\n"));
}

#[tokio::test]
async fn a_tagged_workspace_converts_and_stays_sidecar_across_a_reopen() {
    let dir = tempfile::tempdir().unwrap();
    seed(dir.path());
    let mut ws = open(dir.path());
    assert_eq!(ws.identity_mode(), IdentityMode::Tagged);

    let handles = ws.begin_sidecar_migration().unwrap();
    let report = migrate_documents(handles, false).await;

    assert!(report.failures.is_empty(), "{:?}", report.failures);
    assert_eq!((report.files, report.tasks, report.stripped), (2, 3, 3));
    assert_eq!(ws.identity_mode(), IdentityMode::Sidecar);
    assert_eq!(read(&dir.path().join("todo.txt")), "one\ntwo\n");
    assert_eq!(read(&dir.path().join("q4/todo.txt")), "three\n");
    drop(ws);

    let reopened = open(dir.path());
    assert_eq!(reopened.identity_mode(), IdentityMode::Sidecar);
    assert_eq!(read(&dir.path().join("todo.txt")), "one\ntwo\n");
}

#[tokio::test]
async fn a_dry_run_changes_nothing_and_leaves_the_mode_alone() {
    let dir = tempfile::tempdir().unwrap();
    seed(dir.path());
    let ws = open(dir.path());
    let before = read(&dir.path().join("todo.txt"));

    let report = migrate_documents(ws.document_handles(), true).await;

    assert_eq!((report.files, report.tasks, report.stripped), (2, 3, 3));
    assert_eq!(ws.identity_mode(), IdentityMode::Tagged);
    assert_eq!(read(&dir.path().join("todo.txt")), before);
}

#[tokio::test]
async fn a_second_run_finds_nothing_left_to_strip() {
    let dir = tempfile::tempdir().unwrap();
    seed(dir.path());
    let mut ws = open(dir.path());
    let first = ws.begin_sidecar_migration().unwrap();
    migrate_documents(first, false).await;

    let again = ws.begin_sidecar_migration().unwrap();
    let report = migrate_documents(again, false).await;

    assert!(report.failures.is_empty());
    assert_eq!(report.stripped, 0);
}
