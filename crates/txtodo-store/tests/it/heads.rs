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
fn ops_for_returns_a_devices_run_in_its_number_order() {
    let dir = tempfile::tempdir().unwrap();
    let store = seeded(dir.path());
    let ids = |run: &[txtodo_store::Stored]| -> Vec<u128> {
        run.iter().map(|s| s.op.id.ulid().to_u128()).collect()
    };
    assert_eq!(ids(&store.ops_for(dev(1), 1, 3).unwrap()), vec![1, 3, 5]);
    assert_eq!(ids(&store.ops_for(dev(1), 2, 2).unwrap()), vec![3]);
    assert_eq!(
        ids(&store.ops_for(dev(2), 1, 2).unwrap()),
        vec![2, 4],
        "numbered as they were written, not by HLC (ADR 0039)"
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
        conn.execute_batch(include_str!("../../migrations/0001.sql"))
            .unwrap();
        let v: i64 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(v, 1);
    }
    let mut store = Store::open(&path).unwrap();
    assert_eq!(store.user_version().unwrap(), 9);
    store.append(&[op(9, dev(9), 5)]).unwrap();
    assert_eq!(store.head_of(dev(9)).unwrap(), 1);
    let again = Store::open(&path).unwrap();
    assert_eq!(again.user_version().unwrap(), 9, "idempotent");
}

/// ADR 0039: a later op of ours that sorts before ops already sent (another file actor's clock)
/// takes the next number; the numbers already handed out do not move.
#[test]
fn a_later_op_with_an_older_stamp_takes_the_next_number_and_shifts_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = open(dir.path());
    store
        .append(&[op(1, dev(1), 500), op(2, dev(1), 600)])
        .unwrap();
    let before: Vec<u128> = store
        .ops_for(dev(1), 1, 2)
        .unwrap()
        .iter()
        .map(|s| s.op.id.ulid().to_u128())
        .collect();

    store.append(&[op(3, dev(1), 100)]).unwrap();

    let ids = |first, last| -> Vec<u128> {
        store
            .ops_for(dev(1), first, last)
            .unwrap()
            .iter()
            .map(|s| s.op.id.ulid().to_u128())
            .collect()
    };
    assert_eq!(ids(1, 2), before, "1 and 2 are still the same ops");
    assert_eq!(ids(3, 3), vec![3]);
    assert_eq!(store.head_of(dev(1)).unwrap(), 3);
}

/// A peer's ops take the numbers their sync batch gave them.
#[test]
fn a_peers_ops_keep_the_numbers_they_came_with() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = open(dir.path());
    let projection = txtodo_store::Projection {
        file: FilePath::new("todo.txt").unwrap(),
        bytes: Vec::new(),
        hash: [0; 32],
        written_at_ms: 1,
    };
    store
        .commit_change_numbered(
            (&[op(1, dev(2), 900), op(2, dev(2), 100)], &[1, 2]),
            &projection,
            None,
            &txtodo_store::CommitExtras::default(),
        )
        .unwrap();
    let run: Vec<u128> = store
        .ops_for(dev(2), 1, 2)
        .unwrap()
        .iter()
        .map(|s| s.op.id.ulid().to_u128())
        .collect();
    assert_eq!(run, vec![1, 2]);
}

/// Migration 0009 numbers a version-8 log by today's rank: HLC order per device.
#[test]
fn a_version_eight_log_is_numbered_by_its_hlc_rank() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("oplog.db");
    {
        let conn = rusqlite::Connection::open(&path).unwrap();
        for sql in [
            include_str!("../../migrations/0001.sql"),
            include_str!("../../migrations/0002.sql"),
            include_str!("../../migrations/0003.sql"),
            include_str!("../../migrations/0004.sql"),
            include_str!("../../migrations/0005.sql"),
            include_str!("../../migrations/0006.sql"),
            include_str!("../../migrations/0007.sql"),
            include_str!("../../migrations/0008.sql"),
        ] {
            conn.execute_batch(sql).unwrap();
        }
        // Written in this order; device 1's HLC order is 3, 1, 2.
        for (n, wall) in [(1u128, 200u64), (2, 300), (3, 100)] {
            let o = op(n, dev(1), wall);
            conn.execute(
                "INSERT INTO ops (op_id, hlc_wall, hlc_counter, device, principal, file, kind, payload) \
                 VALUES (?1, ?2, 0, ?3, 'external', 'todo.txt', 'blank_insert', ?4)",
                rusqlite::params![
                    n.to_be_bytes().to_vec(),
                    wall as i64,
                    dev(1).ulid().to_u128().to_be_bytes().to_vec(),
                    postcard::to_allocvec(&o).unwrap(),
                ],
            )
            .unwrap();
        }
    }
    let store = Store::open(&path).unwrap();
    let run: Vec<u128> = store
        .ops_for(dev(1), 1, 3)
        .unwrap()
        .iter()
        .map(|s| s.op.id.ulid().to_u128())
        .collect();
    assert_eq!(run, vec![3, 1, 2]);
    assert_eq!(store.head_of(dev(1)).unwrap(), 3);
}
