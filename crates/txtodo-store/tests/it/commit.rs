//! commit_change atomicity and the reads undo/checkout use.
// Integration tests are tests: clippy.toml allows unwrap/expect in #[test] fns but not in their helpers.
#![allow(clippy::expect_used, clippy::unwrap_used)]

use txtodo_model::{DeviceId, FilePath, Hlc, Op, OpId, OpKind, Principal, TaskId, Ulid};
use txtodo_store::{Projection, Seq, Snapshot, Store, StoreError};

fn device() -> DeviceId {
    DeviceId::new(Ulid::from_u128(7))
}

fn op(n: u128, wall_ms: u64) -> Op {
    Op {
        id: OpId::new(Ulid::from_u128(n)),
        hlc: Hlc {
            wall_ms,
            counter: 0,
            device: device(),
        },
        principal: Principal::User { device: device() },
        file: FilePath::new("todo.txt").unwrap(),
        kind: OpKind::BlankInsert {
            after: Some(TaskId::new(Ulid::from_u128(n))),
        },
    }
}

fn projection(bytes: &[u8], n: u8) -> Projection {
    Projection {
        file: FilePath::new("todo.txt").unwrap(),
        bytes: bytes.to_vec(),
        hash: [n; 32],
        written_at_ms: 1,
    }
}

#[test]
fn commit_change_writes_ops_projection_and_prev_hash_together_or_not_at_all() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open(&dir.path().join("oplog.db")).unwrap();
    let todo = FilePath::new("todo.txt").unwrap();
    assert_eq!(store.prev_hash(&todo).unwrap(), None);
    let range = store
        .commit_change(&[op(1, 10), op(2, 11)], &projection(b"two\n", 2), None)
        .unwrap()
        .unwrap();
    assert_eq!((range.first, range.last), (Seq(1), Seq(2)));
    assert_eq!(store.get_projection(&todo).unwrap().unwrap().hash, [2; 32]);
    assert_eq!(
        store.prev_hash(&todo).unwrap(),
        None,
        "empty meta value means no previous hash"
    );
    // Zero ops is fine: projection and prev hash still move.
    assert_eq!(
        store
            .commit_change(&[], &projection(b"three\n", 3), Some([2; 32]))
            .unwrap(),
        None
    );
    assert_eq!(store.prev_hash(&todo).unwrap(), Some([2; 32]));
}

#[test]
fn a_failed_commit_change_rolls_everything_back_and_the_connection_stays_usable() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open(&dir.path().join("oplog.db")).unwrap();
    let todo = FilePath::new("todo.txt").unwrap();
    store
        .commit_change(
            &[op(1, 10), op(2, 11)],
            &projection(b"three\n", 3),
            Some([2; 32]),
        )
        .unwrap();
    // A duplicate op id fails the whole commit: projection and prev hash stay at "three".
    let err = store
        .commit_change(
            &[op(3, 12), op(1, 13)],
            &projection(b"four\n", 4),
            Some([3; 32]),
        )
        .unwrap_err();
    assert!(matches!(
        err,
        StoreError::Sqlite {
            op: "insert op",
            ..
        }
    ));
    assert_eq!(
        store.get_projection(&todo).unwrap().unwrap().bytes,
        b"three\n"
    );
    assert_eq!(store.prev_hash(&todo).unwrap(), Some([2; 32]));
    assert_eq!(store.last_seq().unwrap(), Some(Seq(2)));
    // The connection is usable after the rollback.
    store
        .commit_change(&[op(3, 12)], &projection(b"four\n", 4), Some([3; 32]))
        .unwrap();
    assert_eq!(store.last_seq().unwrap(), Some(Seq(3)));
}

#[test]
fn newest_is_descending_and_snapshot_lookup_is_at_or_before() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open(&dir.path().join("oplog.db")).unwrap();
    let todo = FilePath::new("todo.txt").unwrap();
    store
        .append(&[op(1, 1), op(2, 2), op(3, 3), op(4, 4)])
        .unwrap();
    let newest = store.newest(&todo, 2).unwrap();
    assert_eq!(
        newest.iter().map(|s| s.seq).collect::<Vec<_>>(),
        vec![Seq(4), Seq(3)]
    );
    assert_eq!(store.newest(&todo, 0).unwrap().len(), 0);
    store
        .put_snapshot(
            &todo,
            &Snapshot {
                seq: Seq(2),
                state: b"at two".to_vec(),
            },
        )
        .unwrap();
    store
        .put_snapshot(
            &todo,
            &Snapshot {
                seq: Seq(4),
                state: b"at four".to_vec(),
            },
        )
        .unwrap();
    assert_eq!(store.snapshot_at_or_before(&todo, Seq(1)).unwrap(), None);
    assert_eq!(
        store
            .snapshot_at_or_before(&todo, Seq(3))
            .unwrap()
            .unwrap()
            .seq,
        Seq(2)
    );
    assert_eq!(
        store
            .snapshot_at_or_before(&todo, Seq(4))
            .unwrap()
            .unwrap()
            .state,
        b"at four"
    );
    assert_eq!(
        store
            .snapshot_at_or_before(&todo, Seq(99))
            .unwrap()
            .unwrap()
            .seq,
        Seq(4)
    );
}
