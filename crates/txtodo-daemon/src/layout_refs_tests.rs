//! Task `workspace-layout`: under the default layout the root list's `ref:` directories live in
//! `tasks/`, a nested list's stay beside it, and moving a line between the two lands the directory
//! in the right place.

use crate::actor::{ActorConfig, FileActor, SharedStore};
use crate::clock::FakeClock;
use crate::layout_state::SharedLayout;
use crate::move_coordinator::move_task_across_files;
use crate::mutation::{Mutation, TaskRef};
use crate::stats::Stats;
use std::path::Path;
use std::sync::{Arc, Mutex};
use txtodo_model::{DeviceId, FilePath, IdentityMode, Principal, TaskId, Ulid, WorkspaceLayout};
use txtodo_store::Store;

const ID: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";

fn device() -> DeviceId {
    DeviceId::new(Ulid::from_u128(9))
}

fn user() -> Principal {
    Principal::User { device: device() }
}

fn store(root: &Path) -> SharedStore {
    Arc::new(Mutex::new(Store::open(&root.join("oplog.db")).unwrap()))
}

fn clock() -> Arc<dyn crate::clock::Clock> {
    // One clock for every actor sharing a store: separate ones mint the same op ids.
    static CLOCK: std::sync::OnceLock<Arc<dyn crate::clock::Clock>> = std::sync::OnceLock::new();
    Arc::clone(CLOCK.get_or_init(|| Arc::new(FakeClock::new(1_000))))
}

fn open_at(root: &Path, rel: &str, store: &SharedStore, layout: &SharedLayout) -> FileActor {
    let cfg = ActorConfig {
        path: FilePath::new(rel).unwrap(),
        disk: root.join(rel),
        device: device(),
        stats: Arc::new(Stats::default()),
        identity_mode: IdentityMode::Tagged,
        tree_dirty: Arc::new(crate::tree_dirty::TreeDirty::default()),
        layout: layout.clone(),
    };
    FileActor::open(cfg, Arc::clone(store), clock()).unwrap()
}

fn tasks_layout() -> SharedLayout {
    SharedLayout::new(WorkspaceLayout::default())
}

fn first_line(id: Option<&str>) -> TaskRef {
    TaskRef {
        line_number: 1,
        task_id: id.map(|i| TaskId::new(Ulid::parse(i).unwrap())),
    }
}

#[tokio::test]
async fn a_root_line_gets_its_ref_dir_under_tasks() {
    let dir = tempfile::tempdir().unwrap();
    let handle = open_at(dir.path(), "todo.txt", &store(dir.path()), &tasks_layout()).spawn();
    handle
        .apply(
            vec![Mutation::Add {
                line: "(A) Q4 roadmap +work".into(),
            }],
            user(),
        )
        .await
        .unwrap();
    let info = handle
        .ensure_ref_dir(first_line(None), user())
        .await
        .unwrap();
    assert_eq!(info.slug, "q4-roadmap");
    assert!(dir.path().join("tasks/q4-roadmap").is_dir(), "{info:?}");
    assert!(
        !dir.path().join("q4-roadmap").exists(),
        "nothing beside the list"
    );
}

#[tokio::test]
async fn a_nested_list_keeps_its_ref_dirs_beside_itself() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("tasks/plan")).unwrap();
    let nested = open_at(
        dir.path(),
        "tasks/plan/todo.txt",
        &store(dir.path()),
        &tasks_layout(),
    );
    let handle = nested.spawn();
    handle
        .apply(
            vec![Mutation::Add {
                line: "draft outline".into(),
            }],
            user(),
        )
        .await
        .unwrap();
    let info = handle
        .ensure_ref_dir(first_line(None), user())
        .await
        .unwrap();
    assert!(
        dir.path().join("tasks/plan/draft-outline").is_dir(),
        "{info:?}"
    );
    assert!(!dir.path().join("tasks/plan/tasks").exists());
}

#[tokio::test]
async fn moving_a_line_between_the_root_list_and_a_ref_list_relocates_its_dir() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::write(
        root.join("todo.txt"),
        format!("(A) roadmap ref:project id:{ID}\n"),
    )
    .unwrap();
    std::fs::create_dir_all(root.join("tasks/project")).unwrap();
    std::fs::write(root.join("tasks/project/notes.md"), b"hello").unwrap();
    std::fs::create_dir_all(root.join("tasks/other")).unwrap();
    let (store, layout) = (store(root), tasks_layout());
    let source = open_at(root, "todo.txt", &store, &layout).spawn();
    let dest = open_at(root, "tasks/other/todo.txt", &store, &layout).spawn();

    // Root list -> a nested list: the dir leaves `tasks/` and sits beside the nested file.
    move_task_across_files(
        &source,
        &dest,
        first_line(Some(ID)),
        user(),
        (root, &layout.get()),
    )
    .await
    .unwrap();
    assert!(!root.join("tasks/project").exists());
    assert!(root.join("tasks/other/project/notes.md").is_file());

    // And back: from the nested list to the root list, into `tasks/` again.
    move_task_across_files(
        &dest,
        &source,
        first_line(Some(ID)),
        user(),
        (root, &layout.get()),
    )
    .await
    .unwrap();
    assert!(!root.join("tasks/other/project").exists());
    assert!(root.join("tasks/project/notes.md").is_file());
}
