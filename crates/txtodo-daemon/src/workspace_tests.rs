//! Workspace: discovery spawns actors, registration is idempotent, later directories join.

use crate::clock::FakeClock;
use crate::workspace::Workspace;
use std::path::Path;
use std::sync::Arc;
use txtodo_model::FilePath;

fn touch(p: &Path, bytes: &str) {
    if let Some(d) = p.parent() {
        std::fs::create_dir_all(d).unwrap_or_else(|e| panic!("{e}"));
    }
    std::fs::write(p, bytes).unwrap_or_else(|e| panic!("{e}"));
}

fn open(root: &Path) -> Workspace {
    Workspace::open(root, Arc::new(FakeClock::new(1_000)))
        .unwrap_or_else(|e| panic!("open workspace: {e}"))
}

#[tokio::test]
async fn open_discovers_every_document_and_mints_one_device_id() {
    let dir = tempfile::tempdir().unwrap();
    touch(&dir.path().join("todo.txt"), "one\n");
    touch(&dir.path().join("q4/todo.txt"), "two\n");
    touch(&dir.path().join("q4/notes.md"), "# notes\n");
    touch(&dir.path().join("q4/other.txt"), "ignored\n");
    let ws = open(dir.path());
    let paths: Vec<String> = ws.paths().map(ToString::to_string).collect();
    assert_eq!(
        paths,
        vec!["q4/todo.txt", "todo.txt"],
        "notes.md is not a managed document"
    );
    let got = ws
        .actor(&FilePath::new("q4/todo.txt").unwrap())
        .unwrap()
        .get()
        .await
        .unwrap();
    assert!(got.bytes.starts_with(b"two id:"), "adopted with an id");
    assert!(
        ws.actor_for_disk(&dir.path().join("q4").join("todo.txt"))
            .is_some()
    );
    assert!(
        ws.actor_for_disk(&dir.path().join("q4").join("other.txt"))
            .is_none()
    );
    let device = ws.device();
    drop(ws);
    let again = open(dir.path());
    assert_eq!(again.device(), device, "the device id is persisted in meta");
    assert_eq!(again.started_at_ms(), 1_000);
}

#[tokio::test]
async fn register_is_idempotent_and_discover_picks_up_a_new_directory() {
    let dir = tempfile::tempdir().unwrap();
    touch(&dir.path().join("todo.txt"), "");
    let mut ws = open(dir.path());
    assert!(!ws.register(FilePath::new("todo.txt").unwrap()).unwrap());
    touch(&dir.path().join("later/todo.txt"), "three\n");
    touch(&dir.path().join("later/deep/done.txt"), "");
    assert_eq!(ws.discover(&dir.path().join("later")).unwrap(), 2);
    assert_eq!(
        ws.discover(&dir.path().join("later")).unwrap(),
        0,
        "second pass finds nothing new"
    );
    assert_eq!(ws.paths().count(), 3);
    assert!(
        ws.store().lock().unwrap().last_seq().unwrap().is_some(),
        "adoption wrote ops"
    );
}

// Regression for the line-136 bug: the walker shipped notes.md and the actor stamped `id:` tags
// into prose (DocState is the *task* model). M5 (tasks/daemon-workspace-walker) now discovers
// `notes.md` at the walker level, plan §3.2 rule 11 ("every notes.md ... is a synced document"),
// so a future notes actor (tasks/crdt-notes-doc) can find it — but opening the workspace must
// still leave the file itself untouched: no `FileActor`/`DocState` is built for it here.
#[tokio::test]
async fn notes_md_is_left_alone() {
    let dir = tempfile::tempdir().unwrap();
    let notes = "Some prose about the roadmap.\n\n- a bullet\n";
    touch(&dir.path().join("todo.txt"), "one\n");
    touch(&dir.path().join("notes.md"), notes);
    assert!(
        crate::walker::walk(dir.path())
            .unwrap()
            .iter()
            .any(|p| p.as_str() == "notes.md"),
        "the walker discovers notes.md"
    );
    let ws = open(dir.path());
    let paths: Vec<String> = ws.paths().map(ToString::to_string).collect();
    assert_eq!(
        paths,
        vec!["todo.txt"],
        "notes.md is discovered but gets no FileActor/DocState"
    );
    assert_eq!(
        std::fs::read_to_string(dir.path().join("notes.md")).unwrap(),
        notes,
        "notes.md bytes are byte-identical after open"
    );
}
