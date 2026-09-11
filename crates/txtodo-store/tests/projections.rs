//! Projection cache, snapshots and meta: upsert semantics and round trips.
// Integration tests are tests: clippy.toml allows unwrap/expect in #[test] fns but not in their helpers.
#![allow(clippy::expect_used, clippy::unwrap_used)]

use txtodo_model::FilePath;
use txtodo_store::{MAX_PROJECTION_BYTES, Projection, Seq, Snapshot, Store, StoreError};

fn projection(bytes: &[u8], written_at_ms: u64) -> Projection {
    let mut hash = [0u8; 32];
    hash[..bytes.len().min(32)].copy_from_slice(&bytes[..bytes.len().min(32)]);
    Projection {
        file: FilePath::new("todo.txt").unwrap(),
        bytes: bytes.to_vec(),
        hash,
        written_at_ms,
    }
}

#[test]
fn projection_upserts_and_reads_back_exactly() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open(&dir.path().join("oplog.db")).unwrap();
    let todo = FilePath::new("todo.txt").unwrap();
    assert_eq!(store.get_projection(&todo).unwrap(), None);
    let first = projection(b"(A) one\r\n", 1_000);
    store.put_projection(&first).unwrap();
    assert_eq!(store.get_projection(&todo).unwrap(), Some(first));
    let second = projection(b"(A) one\r\nx two\r\n", 2_000);
    store.put_projection(&second).unwrap();
    assert_eq!(
        store.get_projection(&todo).unwrap(),
        Some(second),
        "replaced, not appended"
    );
    let huge = Projection {
        bytes: vec![0; MAX_PROJECTION_BYTES + 1],
        ..projection(b"", 0)
    };
    assert!(matches!(
        store.put_projection(&huge),
        Err(StoreError::ProjectionTooLarge(_))
    ));
}

#[test]
fn latest_snapshot_is_the_highest_seq_and_meta_round_trips() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open(&dir.path().join("oplog.db")).unwrap();
    let todo = FilePath::new("todo.txt").unwrap();
    let done = FilePath::new("done.txt").unwrap();
    assert_eq!(store.latest_snapshot(&todo).unwrap(), None);
    store
        .put_snapshot(
            &todo,
            &Snapshot {
                seq: Seq(10),
                state: b"ten".to_vec(),
            },
        )
        .unwrap();
    store
        .put_snapshot(
            &todo,
            &Snapshot {
                seq: Seq(5),
                state: b"five".to_vec(),
            },
        )
        .unwrap();
    store
        .put_snapshot(
            &done,
            &Snapshot {
                seq: Seq(99),
                state: b"other file".to_vec(),
            },
        )
        .unwrap();
    assert_eq!(
        store.latest_snapshot(&todo).unwrap(),
        Some(Snapshot {
            seq: Seq(10),
            state: b"ten".to_vec()
        })
    );
    store
        .put_snapshot(
            &todo,
            &Snapshot {
                seq: Seq(10),
                state: b"ten again".to_vec(),
            },
        )
        .unwrap();
    assert_eq!(
        store.latest_snapshot(&todo).unwrap().unwrap().state,
        b"ten again",
        "same seq replaces"
    );

    assert_eq!(store.meta_get("device_id").unwrap(), None);
    store.meta_set("device_id", &[1, 2, 3]).unwrap();
    store.meta_set("device_id", &[4, 5]).unwrap();
    assert_eq!(store.meta_get("device_id").unwrap(), Some(vec![4, 5]));
}
