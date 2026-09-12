//! Per-device heads and runs: counts per device, runs in that device's HLC order, refused ranges,
//! and the 0001 → 0002 migration on an existing database.
// Integration tests are tests: clippy.toml allows unwrap/expect in #[test] fns but not in their helpers.
#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::path::Path;

use txtodo_model::{DeviceId, FilePath, Hlc, Op, OpId, OpKind, Principal, TaskId, Ulid};
use txtodo_store::{Store, StoreError};

fn dev(n: u128) -> DeviceId {
    DeviceId::new(Ulid::from_u128(n))
}

fn op(n: u128, device: DeviceId, wall_ms: u64) -> Op {
    Op {
        id: OpId::new(Ulid::from_u128(n)),
        hlc: Hlc {
            wall_ms,
            counter: 0,
            device,
        },
        principal: Principal::External { device },
        file: FilePath::new("todo.txt").unwrap(),
        kind: OpKind::BlankInsert {
            after: Some(TaskId::new(Ulid::from_u128(n))),
        },
    }
}

fn open(dir: &Path) -> Store {
    Store::open(&dir.join("oplog.db")).unwrap_or_else(|e| panic!("open: {e}"))
}

/// Two devices, interleaved by wall time and appended out of HLC order for device 2.
fn seeded(dir: &Path) -> Store {
    let mut store = open(dir);
    store
        .append(&[
            op(1, dev(1), 100),
            op(2, dev(2), 150),
            op(3, dev(1), 200),
            op(4, dev(2), 120), // older than op 2 on the same device: HLC order, not seq order
            op(5, dev(1), 300),
        ])
        .unwrap();
    store
}

#[test]
fn heads_count_each_devices_ops_and_next_origin_seq_is_one_past() {
    let dir = tempfile::tempdir().unwrap();
    let store = seeded(dir.path());
    let heads = store.heads().unwrap();
    assert_eq!(heads.get(&dev(1)), Some(&3));
    assert_eq!(heads.get(&dev(2)), Some(&2));
    assert_eq!(heads.len(), 2);
    assert_eq!(store.head_of(dev(3)).unwrap(), 0, "never heard from");
    assert_eq!(store.next_origin_seq(dev(1)).unwrap(), 4);
    assert_eq!(store.next_origin_seq(dev(3)).unwrap(), 1);
    let empty = tempfile::tempdir().unwrap();
    assert!(open(empty.path()).heads().unwrap().is_empty());
}

#[test]
fn ops_for_returns_a_devices_run_in_its_hlc_order_and_refuses_bad_runs() {
    let dir = tempfile::tempdir().unwrap();
    let store = seeded(dir.path());
    let ids = |run: &[txtodo_store::Stored]| -> Vec<u128> {
        run.iter().map(|s| s.op.id.ulid().to_u128()).collect()
    };
    assert_eq!(ids(&store.ops_for(dev(1), 1, 3).unwrap()), vec![1, 3, 5]);
    assert_eq!(ids(&store.ops_for(dev(1), 2, 2).unwrap()), vec![3]);
    assert_eq!(
        ids(&store.ops_for(dev(2), 1, 2).unwrap()),
        vec![4, 2],
        "device 2's ops come back in HLC order, not append order"
    );
    assert!(
        store.ops_for(dev(1), 3, 9).unwrap().len() == 1,
        "past the head comes back short, not an error"
    );
}

#[test]
fn ops_for_refuses_an_empty_inverted_or_over_wide_run() {
    let dir = tempfile::tempdir().unwrap();
    let store = seeded(dir.path());
    assert!(matches!(
        store.ops_for(dev(1), 0, 1),
        Err(StoreError::BadRun { first: 0, last: 1 })
    ));
    assert!(matches!(
        store.ops_for(dev(1), 3, 2),
        Err(StoreError::BadRun { .. })
    ));
    assert!(matches!(
        store.ops_for(dev(1), 1, 1 + txtodo_store::MAX_OPS_PER_READ as u64),
        Err(StoreError::BadRun { .. })
    ));
}

#[test]
fn a_version_one_database_migrates_to_two_and_keeps_its_rows() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("oplog.db");
    {
        // A pre-M4 file: only 0001 applied, one row in it.
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute_batch(include_str!("../migrations/0001.sql"))
            .unwrap();
        let v: i64 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(v, 1);
    }
    let mut store = Store::open(&path).unwrap();
    assert_eq!(store.user_version().unwrap(), 2);
    store.append(&[op(9, dev(9), 5)]).unwrap();
    assert_eq!(store.head_of(dev(9)).unwrap(), 1);
    let again = Store::open(&path).unwrap();
    assert_eq!(again.user_version().unwrap(), 2, "idempotent");
}
