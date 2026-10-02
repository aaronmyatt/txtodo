//! Task partition-converge line 12 (chaos 20261001-233439, token a1r0n3 lost on all three): under
//! Sidecar identity a line carries no `id:`, so a save racing a peer's add is matched by content
//! and position. The editor's appended line must not take over the peer's just-synced one.

use crate::actor::{ActorConfig, FileActor, SharedStore};
use crate::clock::FakeClock;
use crate::handle::ActorHandle;
use crate::stats::Stats;
use std::path::Path;
use std::sync::{Arc, Mutex};
use txtodo_model::Ulid;
use txtodo_model::{DeviceId, FilePath, Hlc, IdentityMode, Op, OpId, OpKind, Principal, TaskId};
use txtodo_store::Store;

fn cfg(dir: &Path) -> ActorConfig {
    ActorConfig {
        path: FilePath::new("todo.txt").unwrap(),
        disk: dir.join("todo.txt"),
        device: DeviceId::new(Ulid::from_u128(7)),
        stats: Arc::new(Stats::default()),
        identity_mode: IdentityMode::Sidecar,
        tree_dirty: Arc::new(crate::tree_dirty::TreeDirty::default()),
        layout: crate::layout_state::SharedLayout::default(),
    }
}

fn start(dir: &Path, text: &str) -> ActorHandle {
    std::fs::write(dir.join("todo.txt"), text).unwrap();
    let store: SharedStore = Arc::new(Mutex::new(Store::open(&dir.join("oplog.db")).unwrap()));
    let clock: Arc<dyn crate::clock::Clock> = Arc::new(FakeClock::new(1_000));
    FileActor::open(cfg(dir), store, clock).unwrap().spawn()
}

fn disk(dir: &Path) -> String {
    String::from_utf8(std::fs::read(dir.join("todo.txt")).unwrap()).unwrap()
}

/// What an editor that read `seen` writes back: `seen` plus one line, renamed over the list.
fn editor_saves(dir: &Path, seen: &str, line: &str) {
    let tmp = dir.join(".todo.txt.swp-save");
    std::fs::write(&tmp, format!("{seen}{line}\n")).unwrap();
    std::fs::rename(&tmp, dir.join("todo.txt")).unwrap();
}

/// A sidecar peer's add: the line text has no `id:` tag.
fn peer_add(after: TaskId, line: &str) -> Op {
    let peer = DeviceId::new(Ulid::from_u128(9));
    Op {
        id: OpId::new(Ulid::from_u128(0x6001)),
        hlc: Hlc {
            wall_ms: 900,
            counter: 0,
            device: peer,
        },
        principal: Principal::User { device: peer },
        file: FilePath::new("todo.txt").unwrap(),
        kind: OpKind::Insert {
            task: TaskId::new(Ulid::from_u128(0x5001)),
            after: Some(after),
            line: line.into(),
        },
    }
}

async fn last_task(handle: &ActorHandle) -> TaskId {
    let got = handle.get().await.unwrap();
    got.task_ids.into_iter().flatten().last().unwrap()
}

async fn watcher_event(handle: &ActorHandle) {
    handle.external_change().await.unwrap();
    handle.get().await.unwrap();
}

/// The save is on disk before the peer's add commits: the add's write is held, and the save
/// merges three-way against what we last wrote.
#[tokio::test]
async fn a_held_save_and_a_peers_add_at_the_end_both_stay() {
    let dir = tempfile::tempdir().unwrap();
    let seen = "buy milk +lab\nwalk the dog +lab\n";
    let handle = start(dir.path(), seen);
    let dog = last_task(&handle).await;
    editor_saves(dir.path(), seen, "editor-2 +lab @b1");
    handle
        .sync_import_ops(vec![peer_add(dog, "peer-1 +lab @a1")])
        .await
        .unwrap();
    watcher_event(&handle).await;
    let text = disk(dir.path());
    assert!(
        text.contains("editor-2") && text.contains("peer-1"),
        "{text}"
    );
    assert_eq!(text.lines().count(), 4, "{text}");
    assert_eq!(handle.get().await.unwrap().bytes, text.as_bytes());
}

/// The lab's other window: the peer's add is already written, and an editor that read the file
/// before it renames its save over it (between its last read and its rename). The save is based
/// on bytes we wrote a moment ago, not on the latest: it adds a line, it does not rewrite the
/// peer's.
#[tokio::test]
async fn a_save_based_on_our_previous_write_keeps_the_peers_add() {
    let dir = tempfile::tempdir().unwrap();
    let seen = "buy milk +lab\nwalk the dog +lab\n";
    let handle = start(dir.path(), seen);
    let dog = last_task(&handle).await;
    handle
        .sync_import_ops(vec![peer_add(dog, "peer-1 +lab @a1")])
        .await
        .unwrap();
    assert!(disk(dir.path()).contains("peer-1"), "the add is written");
    editor_saves(dir.path(), seen, "editor-2 +lab @b1");
    watcher_event(&handle).await;
    let text = disk(dir.path());
    assert!(
        text.contains("editor-2") && text.contains("peer-1"),
        "{text}"
    );
    assert_eq!(text.lines().count(), 4, "{text}");
}
