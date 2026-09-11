//! The op log against a real WAL database in a temp dir: migrations, append atomicity, reads.
// Integration tests are tests: clippy.toml allows unwrap/expect in #[test] fns but not in their helpers.
#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::path::Path;

use txtodo_model::{DeviceId, FilePath, Hlc, Op, OpId, OpKind, Principal, TaskId, Ulid};
use txtodo_store::{Seq, Store, StoreError};

fn device() -> DeviceId {
    DeviceId::new(Ulid::from_u128(7))
}

fn op(n: u128, wall_ms: u64, counter: u16, file: &str) -> Op {
    Op {
        id: OpId::new(Ulid::from_u128(n)),
        hlc: Hlc {
            wall_ms,
            counter,
            device: device(),
        },
        principal: Principal::External { device: device() },
        file: FilePath::new(file).unwrap(),
        kind: OpKind::BlankInsert {
            after: Some(TaskId::new(Ulid::from_u128(n))),
        },
    }
}

fn open(dir: &Path) -> Store {
    Store::open(&dir.join("oplog.db")).unwrap_or_else(|e| panic!("open: {e}"))
}

#[test]
fn open_migrates_once_and_is_idempotent_in_wal_mode() {
    let dir = tempfile::tempdir().unwrap();
    let store = open(dir.path());
    assert_eq!(store.user_version().unwrap(), 1);
    assert_eq!(store.journal_mode().unwrap(), "wal");
    drop(store);
    let again = open(dir.path());
    assert_eq!(again.user_version().unwrap(), 1);
    assert_eq!(again.last_seq().unwrap(), None);
}

#[test]
fn append_is_atomic_and_duplicate_op_ids_are_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = open(dir.path());
    let todo = FilePath::new("todo.txt").unwrap();
    let range = store
        .append(&[op(1, 10, 0, "todo.txt"), op(2, 10, 1, "todo.txt")])
        .unwrap();
    assert_eq!((range.first, range.last), (Seq(1), Seq(2)));
    // op 3 is fine, op 1 repeats: the whole batch must roll back.
    let err = store
        .append(&[op(3, 11, 0, "todo.txt"), op(1, 11, 1, "todo.txt")])
        .unwrap_err();
    assert!(
        matches!(
            err,
            StoreError::Sqlite {
                op: "insert op",
                ..
            }
        ),
        "{err}"
    );
    assert_eq!(store.last_seq().unwrap(), Some(Seq(2)));
    assert_eq!(store.for_file(&todo, Seq(0)).unwrap().len(), 2);
    assert!(matches!(store.append(&[]), Err(StoreError::EmptyBatch)));
}

#[test]
fn reads_filter_by_file_and_order_by_seq_or_hlc() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = open(dir.path());
    // Appended out of HLC order on purpose: seq order and HLC order differ.
    store
        .append(&[
            op(1, 30, 0, "todo.txt"),
            op(2, 10, 1, "todo.txt"),
            op(3, 10, 0, "done.txt"),
            op(4, 10, 0, "todo.txt"),
        ])
        .unwrap();
    let todo = FilePath::new("todo.txt").unwrap();
    let since = store.for_file(&todo, Seq(1)).unwrap();
    assert_eq!(
        since.iter().map(|s| s.seq).collect::<Vec<_>>(),
        vec![Seq(2), Seq(4)]
    );
    assert_eq!(
        since[0].op,
        op(2, 10, 1, "todo.txt"),
        "payload decodes to the same op"
    );
    let from = Hlc {
        wall_ms: 10,
        counter: 0,
        device: device(),
    };
    let to = Hlc {
        wall_ms: 20,
        counter: 0,
        device: device(),
    };
    let between = store.between(&todo, &from, &to).unwrap();
    assert_eq!(
        between.iter().map(|s| s.seq).collect::<Vec<_>>(),
        vec![Seq(4), Seq(2)],
        "HLC order, todo.txt only"
    );
    let all = store
        .between(
            &todo,
            &from,
            &Hlc {
                wall_ms: 30,
                counter: 0,
                device: device(),
            },
        )
        .unwrap();
    assert_eq!(all.len(), 3, "inclusive upper bound");
}

#[test]
fn the_crate_has_no_update_or_delete_statement() {
    let sources = [
        include_str!("../src/lib.rs"),
        include_str!("../src/ops.rs"),
        include_str!("../src/error.rs"),
    ];
    for src in sources {
        let upper = src.to_uppercase();
        assert!(!upper.contains("UPDATE "), "found UPDATE in a store source");
        assert!(
            !upper.contains("DELETE FROM"),
            "found DELETE in a store source"
        );
    }
}
