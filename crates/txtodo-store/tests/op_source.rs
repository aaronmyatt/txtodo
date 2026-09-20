//! Task op-source: a row carries the client that made it, local to this log. It is a column, not
//! part of the postcard payload, so the op read back is exactly the op written.
// Integration tests are tests: clippy.toml allows unwrap/expect in #[test] fns but not in their helpers.
#![allow(clippy::expect_used, clippy::unwrap_used)]

use txtodo_model::{DeviceId, FilePath, Hlc, Op, OpId, OpKind, Principal, TaskId, Ulid};
use txtodo_store::{MAX_SOURCE_BYTES, Seq, Store, cap_source};

fn op(n: u128) -> Op {
    let device = DeviceId::new(Ulid::from_u128(7));
    Op {
        id: OpId::new(Ulid::from_u128(n)),
        hlc: Hlc {
            wall_ms: 1_000 + n as u64,
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

#[test]
fn a_source_lands_beside_the_op_and_leaves_the_op_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open(&dir.path().join("oplog.db")).unwrap();
    let a = op(1);
    let range = store
        .append_with_source(std::slice::from_ref(&a), Some("cli"))
        .unwrap();
    let rows = store.for_file(&a.file, Seq(0)).unwrap();
    assert_eq!(rows[0].op, a, "the payload has no source in it");
    let sources = store.sources_between(range.first, range.last).unwrap();
    assert_eq!(sources.get(&range.first).map(String::as_str), Some("cli"));
}

#[test]
fn an_append_that_names_no_source_leaves_the_column_empty() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open(&dir.path().join("oplog.db")).unwrap();
    let plain = store.append(&[op(1)]).unwrap();
    let blank = store.append_with_source(&[op(2)], Some("")).unwrap();
    let last = blank.last;
    assert!(
        store.sources_between(plain.first, last).unwrap().is_empty(),
        "no source and an empty source both read back as absent"
    );
}

#[test]
fn an_unknown_source_is_kept_but_cut_to_the_cap_on_a_char_boundary() {
    let long = "é".repeat(MAX_SOURCE_BYTES); // two bytes each, so the cap lands mid-character if naive
    let capped = cap_source(&long);
    assert!(capped.len() <= MAX_SOURCE_BYTES);
    assert!(long.starts_with(capped));
    assert_eq!(cap_source("cli"), "cli");

    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open(&dir.path().join("oplog.db")).unwrap();
    let range = store.append_with_source(&[op(1)], Some(&long)).unwrap();
    let got = store.sources_between(range.first, range.last).unwrap();
    assert_eq!(got.get(&range.first).map(String::as_str), Some(capped));
}

#[test]
fn a_pre_migration_log_opens_with_every_old_row_sourceless() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("oplog.db");
    {
        // A schema-7 file: every migration but 0008, with one row written the old way.
        let conn = rusqlite::Connection::open(&path).unwrap();
        for sql in [
            include_str!("../migrations/0001.sql"),
            include_str!("../migrations/0002.sql"),
            include_str!("../migrations/0003.sql"),
            include_str!("../migrations/0004.sql"),
            include_str!("../migrations/0005.sql"),
            include_str!("../migrations/0006.sql"),
            include_str!("../migrations/0007.sql"),
        ] {
            conn.execute_batch(sql).unwrap();
        }
        conn.execute(
            "INSERT INTO ops (op_id, hlc_wall, hlc_counter, device, principal, file, kind, payload) \
             VALUES (x'01', 1, 0, x'02', 'user', 'todo.txt', 'blank_insert', x'03')",
            [],
        )
        .unwrap();
    }
    let store = Store::open(&path).unwrap();
    assert_eq!(store.user_version().unwrap(), 8);
    assert!(store.sources_between(Seq(1), Seq(1)).unwrap().is_empty());
}
