//! The mirror snapshot round-trips with its seq, and a commit leaves it alone (ADR 0038: only
//! `notes.md` writes one now).
// Integration tests are tests: clippy.toml allows unwrap/expect in #[test] fns but not in their helpers.
#![allow(clippy::expect_used, clippy::unwrap_used)]

use txtodo_model::FilePath;
use txtodo_store::{Seq, Store};

fn todo() -> FilePath {
    FilePath::new("todo.txt").unwrap()
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
fn a_commit_leaves_an_old_mirror_row_alone() {
    use txtodo_store::{CommitExtras, Projection};
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open(&dir.path().join("oplog.db")).unwrap();
    // An old todo.txt mirror row (ADR 0038): a commit neither reads nor rewrites it.
    store.put_mirror(&todo(), b"old-mirror", Seq(3)).unwrap();
    let projection = Projection {
        file: todo(),
        bytes: b"resolved\n".to_vec(),
        hash: [7; 32],
        written_at_ms: 200,
    };
    let range = store
        .commit_change_with(&[], &projection, None, &CommitExtras::default())
        .unwrap();
    assert!(range.is_none(), "no ops in this commit");
    assert_eq!(
        store.get_projection(&todo()).unwrap().unwrap().bytes,
        b"resolved\n"
    );
    assert_eq!(
        store.get_mirror(&todo()).unwrap(),
        Some((b"old-mirror".to_vec(), Seq(3))),
        "the old row stays as it was"
    );
}
