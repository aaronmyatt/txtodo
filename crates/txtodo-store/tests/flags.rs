//! needs_review flags: raise → open, clear → gone and idempotent, re-raise reopens with new
//! texts; the mirror snapshot round-trips with its seq; the schema is at 5.
// Integration tests are tests: clippy.toml allows unwrap/expect in #[test] fns but not in their helpers.
#![allow(clippy::expect_used, clippy::unwrap_used)]

use txtodo_model::{FilePath, TaskId, Ulid};
use txtodo_store::{ReviewRow, Seq, Store};

fn task(n: u128) -> TaskId {
    TaskId::new(Ulid::from_u128(n))
}

fn todo() -> FilePath {
    FilePath::new("todo.txt").unwrap()
}

fn row(n: u128, raised_at_ms: u64, mine: &str, theirs: &str) -> ReviewRow {
    ReviewRow {
        file: todo(),
        task: task(n),
        raised_at_ms,
        mine: mine.as_bytes().to_vec(),
        theirs: theirs.as_bytes().to_vec(),
    }
}

#[test]
fn raise_lists_the_flag_oldest_first_and_clear_removes_it_idempotently() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open(&dir.path().join("oplog.db")).unwrap();
    assert_eq!(store.user_version().unwrap(), 6);
    store
        .raise_flag(&row(2, 200, "b mine", "b theirs"))
        .unwrap();
    store
        .raise_flag(&row(1, 100, "a mine", "a theirs"))
        .unwrap();
    let open = store.open_flags(&todo()).unwrap();
    assert_eq!(open.len(), 2);
    assert_eq!(
        (open[0].task, open[1].task),
        (task(1), task(2)),
        "oldest first"
    );
    assert_eq!(open[0].mine, b"a mine");
    assert!(
        store
            .open_flags(&FilePath::new("other.txt").unwrap())
            .unwrap()
            .is_empty()
    );
    store.clear_flag(&todo(), task(1), 300).unwrap();
    store.clear_flag(&todo(), task(1), 301).unwrap();
    store.clear_flag(&todo(), task(9), 302).unwrap();
    let open = store.open_flags(&todo()).unwrap();
    assert_eq!(open.len(), 1);
    assert_eq!(open[0].task, task(2));
}

#[test]
fn re_raising_a_cleared_flag_reopens_it_with_the_new_texts() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open(&dir.path().join("oplog.db")).unwrap();
    store
        .raise_flag(&row(1, 100, "old mine", "old theirs"))
        .unwrap();
    store.clear_flag(&todo(), task(1), 150).unwrap();
    assert!(store.open_flags(&todo()).unwrap().is_empty());
    store
        .raise_flag(&row(1, 200, "new mine", "new theirs"))
        .unwrap();
    let open = store.open_flags(&todo()).unwrap();
    assert_eq!(open.len(), 1);
    assert_eq!(open[0].raised_at_ms, 200);
    assert_eq!(open[0].theirs, b"new theirs");
}

#[test]
fn mirror_snapshot_round_trips_with_its_seq_and_replaces_the_previous_one() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open(&dir.path().join("oplog.db")).unwrap();
    assert_eq!(store.get_mirror(&todo()).unwrap(), None);
    store.put_mirror(&todo(), b"loro-bytes-1", Seq(10)).unwrap();
    assert_eq!(
        store.get_mirror(&todo()).unwrap(),
        Some((b"loro-bytes-1".to_vec(), Seq(10)))
    );
    store.put_mirror(&todo(), b"loro-bytes-2", Seq(25)).unwrap();
    assert_eq!(
        store.get_mirror(&todo()).unwrap(),
        Some((b"loro-bytes-2".to_vec(), Seq(25)))
    );
    assert_eq!(
        store
            .get_mirror(&FilePath::new("other.txt").unwrap())
            .unwrap(),
        None
    );
}

#[test]
fn commit_change_with_lands_the_clear_and_the_mirror_with_the_commit() {
    use txtodo_store::{CommitExtras, Projection};
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open(&dir.path().join("oplog.db")).unwrap();
    store.raise_flag(&row(1, 100, "mine", "theirs")).unwrap();
    let projection = Projection {
        file: todo(),
        bytes: b"resolved\n".to_vec(),
        hash: [7; 32],
        written_at_ms: 200,
    };
    let extras = CommitExtras {
        clear: Some((task(1), 200)),
        mirror: Some(b"mirror-at-commit".to_vec()),
        fingerprints: Vec::new(),
    };
    let range = store
        .commit_change_with(&[], &projection, None, &extras)
        .unwrap();
    assert!(range.is_none(), "no ops in this commit");
    assert!(store.open_flags(&todo()).unwrap().is_empty());
    assert_eq!(
        store.get_projection(&todo()).unwrap().unwrap().bytes,
        b"resolved\n"
    );
    assert_eq!(
        store.get_mirror(&todo()).unwrap(),
        Some((b"mirror-at-commit".to_vec(), Seq(0))),
        "no ops yet, so the mirror sits at seq 0"
    );
}
