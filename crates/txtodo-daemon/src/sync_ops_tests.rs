//! `FileActor::on_sync_ops`: a peer's already-formed op lands verbatim (own id, own device),
//! never re-stamped, and a batch that fails partway commits nothing (CLAUDE.md §3's negative
//! space — the store, the projection and the state must all agree afterwards).

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

#[tokio::test]
async fn a_batch_that_fails_partway_commits_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let store = store(dir.path());
    let clock = Arc::new(FakeClock::new(1_000));
    let handle = open(dir.path(), &store, &clock).spawn();

    let good_task = TaskId::new(Ulid::from_u128(1));
    let good = peer_insert(good_task, "first line", 1);
    // Insert after a predecessor that was never itself inserted refuses to apply.
    let bad = Op {
        id: OpId::new(Ulid::from_u128(2)),
        hlc: Hlc {
            wall_ms: 5_001,
            counter: 2,
            device: peer_device(),
        },
        principal: Principal::User {
            device: peer_device(),
        },
        file: FilePath::new("todo.txt").unwrap(),
        kind: OpKind::Insert {
            task: TaskId::new(Ulid::from_u128(3)),
            after: Some(TaskId::new(Ulid::from_u128(999))),
            line: "second line".to_string(),
        },
    };

    let before = crate::actor::hash_of(&handle.get().await.unwrap().bytes);
    let result = handle.sync_import_ops(vec![good, bad]).await;
    assert!(result.is_err());

    let after = handle.get().await.unwrap();
    assert_eq!(after.hash, before, "projection unchanged");
    assert!(
        String::from_utf8_lossy(&after.bytes).is_empty(),
        "neither op landed"
    );
    let rows = store
        .lock()
        .unwrap()
        .for_file(&FilePath::new("todo.txt").unwrap(), txtodo_store::Seq(0))
        .unwrap();
    assert!(rows.is_empty(), "the store holds no partial batch either");
}
