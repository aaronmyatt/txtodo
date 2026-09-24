//! `FileActor::on_sync_ops`: a peer's already-formed op lands verbatim (own id, own device),
//! never re-stamped, and an op that does not fit is skipped but kept in the log (task
//! `sync-poison-op`), including one that only fits once the rest of its commit has applied.

use crate::actor::{ActorConfig, FileActor, SharedStore};
use crate::clock::FakeClock;
use crate::stats::Stats;
use std::path::Path;
use std::sync::{Arc, Mutex};
use txtodo_model::{
    DeviceId, FilePath, Hlc, IdentityMode, Op, OpId, OpKind, Principal, TaskId, Ulid,
};
use txtodo_store::Store;

fn local_device() -> DeviceId {
    DeviceId::new(Ulid::from_u128(1))
}

fn peer_device() -> DeviceId {
    DeviceId::new(Ulid::from_u128(2))
}

fn store(dir: &Path) -> SharedStore {
    Arc::new(Mutex::new(
        Store::open(&dir.join("oplog.db")).unwrap_or_else(|e| panic!("open store: {e}")),
    ))
}

fn cfg(dir: &Path) -> ActorConfig {
    ActorConfig {
        path: FilePath::new("todo.txt").unwrap_or_else(|e| panic!("{e}")),
        disk: dir.join("todo.txt"),
        device: local_device(),
        stats: Arc::new(Stats::default()),
        identity_mode: IdentityMode::Tagged,
        tree_dirty: Arc::new(crate::tree_dirty::TreeDirty::default()),
        layout: crate::layout_state::SharedLayout::default(),
    }
}

fn open(dir: &Path, store: &SharedStore, clock: &Arc<FakeClock>) -> FileActor {
    let clock: Arc<dyn crate::clock::Clock> = clock.clone();
    FileActor::open(cfg(dir), Arc::clone(store), clock)
        .unwrap_or_else(|e| panic!("open actor: {e}"))
}

/// One peer-authored `Insert`, exactly as it would arrive off the wire: its own `OpId`, its own
/// device's `Hlc`, never this actor's.
fn peer_insert(task: TaskId, line: &str, counter: u16) -> Op {
    Op {
        id: OpId::new(Ulid::from_u128(u128::from(counter) + 1000)),
        hlc: Hlc {
            wall_ms: 5_000,
            counter,
            device: peer_device(),
        },
        principal: Principal::User {
            device: peer_device(),
        },
        file: FilePath::new("todo.txt").unwrap(),
        kind: OpKind::Insert {
            task,
            after: None,
            line: line.to_string(),
        },
    }
}

#[tokio::test]
async fn a_peers_op_lands_verbatim_with_its_own_id_and_device() {
    let dir = tempfile::tempdir().unwrap();
    let store = store(dir.path());
    let clock = Arc::new(FakeClock::new(1_000));
    let handle = open(dir.path(), &store, &clock).spawn();

    let task = TaskId::new(Ulid::from_u128(42));
    let op = peer_insert(task, &format!("buy ducks id:{task}"), 1);
    handle.sync_import_ops(vec![op.clone()]).await.unwrap();

    let got = handle.get().await.unwrap();
    assert!(String::from_utf8_lossy(&got.bytes).contains("buy ducks"));

    let rows = store
        .lock()
        .unwrap()
        .for_file(&FilePath::new("todo.txt").unwrap(), txtodo_store::Seq(0))
        .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].op.id, op.id, "the peer's own OpId is preserved");
    assert_eq!(
        rows[0].op.hlc.device,
        peer_device(),
        "the peer's own device is preserved, never re-stamped to this device"
    );
}

/// A peer op stamped `(wall_ms, counter)`: ops of one commit share a stamp.
fn peer_op(n: u128, wall_ms: u64, kind: OpKind) -> Op {
    Op {
        id: OpId::new(Ulid::from_u128(n)),
        hlc: Hlc {
            wall_ms,
            counter: 0,
            device: peer_device(),
        },
        principal: Principal::User {
            device: peer_device(),
        },
        file: FilePath::new("todo.txt").unwrap(),
        kind,
    }
}

fn insert(task: TaskId, after: Option<TaskId>, name: &str) -> OpKind {
    OpKind::Insert {
        task,
        after,
        line: format!("{name} id:{task}"),
    }
}

fn rows(store: &SharedStore) -> usize {
    store
        .lock()
        .unwrap()
        .for_file(&FilePath::new("todo.txt").unwrap(), txtodo_store::Seq(0))
        .unwrap()
        .len()
}

#[tokio::test]
async fn an_op_that_does_not_fit_is_kept_in_the_log_and_skipped() {
    let dir = tempfile::tempdir().unwrap();
    let store = store(dir.path());
    let clock = Arc::new(FakeClock::new(1_000));
    let handle = open(dir.path(), &store, &clock).spawn();

    let good = peer_op(
        1,
        5_000,
        insert(TaskId::new(Ulid::from_u128(1)), None, "first"),
    );
    // Anchored after a task no op ever inserted (task sync-poison-op): it can never apply.
    let bad = peer_op(
        2,
        5_001,
        insert(
            TaskId::new(Ulid::from_u128(3)),
            Some(TaskId::new(Ulid::from_u128(999))),
            "second",
        ),
    );
    let after_it = peer_op(
        3,
        5_002,
        insert(TaskId::new(Ulid::from_u128(4)), None, "third"),
    );
    handle
        .sync_import_ops(vec![good, bad, after_it])
        .await
        .unwrap();

    let text = String::from_utf8_lossy(&handle.get().await.unwrap().bytes).into_owned();
    assert!(text.contains("first") && text.contains("third"), "{text}");
    assert!(!text.contains("second"), "{text}");
    assert_eq!(
        rows(&store),
        3,
        "all three are in the log, so heads stay dense"
    );
}

#[tokio::test]
async fn a_move_after_a_task_its_own_commit_inserts_later_applies_once_that_insert_has() {
    let dir = tempfile::tempdir().unwrap();
    let store = store(dir.path());
    let clock = Arc::new(FakeClock::new(1_000));
    let handle = open(dir.path(), &store, &clock).spawn();
    let (a, b, c) = (
        TaskId::new(Ulid::from_u128(1)),
        TaskId::new(Ulid::from_u128(2)),
        TaskId::new(Ulid::from_u128(3)),
    );
    handle
        .sync_import_ops(vec![
            peer_op(1, 5_000, insert(a, None, "a")),
            peer_op(2, 5_000, insert(b, Some(a), "b")),
        ])
        .await
        .unwrap();
    // One commit (one stamp), in the order an old reconciler wrote it: the move names `c`
    // before the insert of `c` (seq 23778 in this repo's own log).
    let to_file = FilePath::new("todo.txt").unwrap();
    handle
        .sync_import_ops(vec![
            peer_op(
                3,
                6_000,
                OpKind::Move {
                    task: a,
                    after: Some(c),
                    to_file,
                },
            ),
            peer_op(4, 6_000, insert(c, Some(b), "c")),
        ])
        .await
        .unwrap();

    let text = String::from_utf8_lossy(&handle.get().await.unwrap().bytes).into_owned();
    let order: Vec<&str> = text.lines().map(|l| &l[..1]).collect();
    assert_eq!(order, vec!["b", "c", "a"], "{text}");
    assert_eq!(rows(&store), 4);
}
