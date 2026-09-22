//! Cross-file `Move`: the line lands at the destination, its `ref:` directory follows (applying
//! the collision rule there), and a failure partway rolls the whole operation back.

use crate::actor::{ActorConfig, FileActor, SharedStore};
use crate::clock::FakeClock;
use crate::move_coordinator::move_task_across_files;
use crate::mutation::TaskRef;
use crate::stats::Stats;
use std::path::Path;
use std::sync::{Arc, Mutex};
use txtodo_model::{DeviceId, FilePath, IdentityMode, Principal, TaskId, Ulid};
use txtodo_store::Store;

const ID: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAA";

fn device() -> DeviceId {
    DeviceId::new(Ulid::from_u128(11))
}

/// A move by `user()` with no client named — the shape every test here needs.
fn origin() -> crate::move_coordinator::Origin {
    crate::move_coordinator::Origin {
        principal: user(),
        source: None,
    }
}

fn user() -> Principal {
    Principal::User { device: device() }
}

fn shared_store(root: &Path) -> SharedStore {
    Arc::new(Mutex::new(
        Store::open(&root.join("oplog.db")).unwrap_or_else(|e| panic!("open store: {e}")),
    ))
}

fn open_at(
    root: &Path,
    rel: &str,
    store: &SharedStore,
    clock: &Arc<dyn crate::clock::Clock>,
) -> FileActor {
    let cfg = ActorConfig {
        path: FilePath::new(rel).unwrap_or_else(|e| panic!("{e}")),
        disk: root.join(rel),
        device: device(),
        stats: Arc::new(Stats::default()),
        identity_mode: IdentityMode::Tagged,
        tree_dirty: Arc::new(crate::tree_dirty::TreeDirty::default()),
        layout: crate::layout_state::SharedLayout::default(),
    };
    FileActor::open(cfg, Arc::clone(store), Arc::clone(clock)).unwrap_or_else(|e| panic!("{e}"))
}

fn fake_clock() -> Arc<dyn crate::clock::Clock> {
    Arc::new(FakeClock::new(1_000))
}

fn read(root: &Path, rel: &str) -> String {
    std::fs::read_to_string(root.join(rel)).unwrap_or_default()
}

fn task_ref() -> TaskRef {
    TaskRef {
        line_number: 1,
        task_id: Some(TaskId::new(
            Ulid::parse(ID).unwrap_or_else(|| panic!("bad ulid")),
        )),
    }
}

#[tokio::test]
async fn move_relocates_the_line_and_its_ref_directory() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::write(
        root.join("todo.txt"),
        format!("(A) roadmap ref:project id:{ID}\n"),
    )
    .unwrap();
    std::fs::create_dir(root.join("project")).unwrap();
    std::fs::write(root.join("project").join("notes.md"), b"hello").unwrap();
    let store = shared_store(root);
    let clock = fake_clock();
    let source = open_at(root, "todo.txt", &store, &clock).spawn();
    let dest = open_at(root, "sub/other.txt", &store, &clock).spawn();
    move_task_across_files(
        &source,
        &dest,
        task_ref(),
        origin(),
        (root, &txtodo_model::WorkspaceLayout::beside_the_list()),
    )
    .await
    .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(read(root, "todo.txt"), "", "the line left the source");
    let moved = read(root, "sub/other.txt");
    assert!(
        moved.contains("ref:project") && moved.contains(ID),
        "{moved}"
    );
    assert!(
        !root.join("project").exists(),
        "the directory left its old spot"
    );
    assert!(root.join("sub/project").is_dir());
    assert_eq!(read(root, "sub/project/notes.md"), "hello");
}

#[tokio::test]
async fn a_slug_collision_at_the_destination_gets_dash_2() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::write(
        root.join("todo.txt"),
        format!("(A) roadmap ref:project id:{ID}\n"),
    )
    .unwrap();
    std::fs::create_dir(root.join("project")).unwrap();
    std::fs::create_dir_all(root.join("sub/project")).unwrap(); // already taken at the destination
    let store = shared_store(root);
    let clock = fake_clock();
    let source = open_at(root, "todo.txt", &store, &clock).spawn();
    let dest = open_at(root, "sub/other.txt", &store, &clock).spawn();
    move_task_across_files(
        &source,
        &dest,
        task_ref(),
        origin(),
        (root, &txtodo_model::WorkspaceLayout::beside_the_list()),
    )
    .await
    .unwrap_or_else(|e| panic!("{e}"));
    assert!(read(root, "sub/other.txt").contains("ref:project-2"));
    assert!(root.join("sub/project-2").is_dir());
    assert!(root.join("sub/project").is_dir(), "left alone, not ours");
}

// Simulated failure (daemon-ref-move.md): the whole operation rolls back and the source is
// byte-identical. Forced here by making the destination's directory unwritable before the move,
// so the destination half of the operation never lands.
#[cfg(unix)]
#[tokio::test]
async fn a_failed_move_rolls_back_and_the_source_is_byte_identical() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let before = format!("(A) roadmap ref:project id:{ID}\n");
    std::fs::write(root.join("todo.txt"), &before).unwrap();
    std::fs::create_dir(root.join("project")).unwrap();
    std::fs::create_dir(root.join("sub")).unwrap();
    std::fs::write(root.join("sub/other.txt"), "").unwrap();
    let store = shared_store(root);
    let clock = fake_clock();
    let source = open_at(root, "todo.txt", &store, &clock).spawn();
    let dest = open_at(root, "sub/other.txt", &store, &clock).spawn();
    std::fs::set_permissions(root.join("sub"), std::fs::Permissions::from_mode(0o555)).unwrap();
    let result = move_task_across_files(
        &source,
        &dest,
        task_ref(),
        origin(),
        (root, &txtodo_model::WorkspaceLayout::beside_the_list()),
    )
    .await;
    std::fs::set_permissions(root.join("sub"), std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(result.is_err(), "{result:?}");
    assert_eq!(
        read(root, "todo.txt"),
        before,
        "the source is exactly as it was"
    );
    assert!(
        root.join("project").is_dir(),
        "its ref: directory never left"
    );
}

/// Task op-source-gaps: the client named by the request is stamped on every commit a cross-file
/// move makes — the source's departure and the destination's insert — so `txtodo log` and the
/// activity pane show `mcp`, not a hole, for a moved task.
#[tokio::test]
async fn a_move_records_the_requests_source_on_both_documents() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::write(root.join("todo.txt"), format!("(A) roadmap id:{ID}\n")).unwrap();
    let store = shared_store(root);
    let clock = fake_clock();
    let source = open_at(root, "todo.txt", &store, &clock).spawn();
    let dest = open_at(root, "sub/other.txt", &store, &clock).spawn();
    let before = store
        .lock()
        .unwrap()
        .last_seq()
        .unwrap()
        .unwrap_or(txtodo_store::Seq(0));
    move_task_across_files(
        &source,
        &dest,
        task_ref(),
        crate::move_coordinator::Origin {
            principal: user(),
            source: Some("mcp".to_owned()),
        },
        (root, &txtodo_model::WorkspaceLayout::beside_the_list()),
    )
    .await
    .unwrap_or_else(|e| panic!("{e}"));
    let guard = store.lock().unwrap();
    let last = guard.last_seq().unwrap().unwrap_or(before);
    let sources = guard
        .sources_between(txtodo_store::Seq(before.0 + 1), last)
        .unwrap();
    assert!(
        sources.len() >= 2 && sources.values().all(|s| s == "mcp"),
        "every op the move appended names the client: {sources:?}"
    );
}
