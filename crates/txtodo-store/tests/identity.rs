//! Sidecar identity fingerprints: upsert → live, retire → tombstoned (idempotent, kept not
//! deleted), a later upsert revives a tombstoned row, files don't see each other's rows, a commit
//! or a retain retires the live rows it leaves out, and the schema lands at 5.
// Integration tests are tests: clippy.toml allows unwrap/expect in #[test] fns but not in their helpers.
#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::collections::BTreeSet;
use std::path::Path;

use txtodo_model::{FilePath, Fingerprint, TaskId, Ulid};
use txtodo_store::Store;

fn open(dir: &Path) -> Store {
    Store::open(&dir.join("oplog.db")).unwrap_or_else(|e| panic!("open: {e}"))
}

fn task(n: u128) -> TaskId {
    TaskId::new(Ulid::from_u128(n))
}

fn todo() -> FilePath {
    FilePath::new("todo.txt").unwrap()
}

fn fp(desc: &str, line_index: usize) -> Fingerprint {
    Fingerprint {
        creation_date: Some((2026, 9, 13)),
        projects: BTreeSet::from(["home".to_owned()]),
        contexts: BTreeSet::from(["errand".to_owned()]),
        description_norm: desc.to_owned(),
        line_index,
    }
}

#[test]
fn migrating_to_fingerprints_lands_the_schema_at_five() {
    let dir = tempfile::tempdir().unwrap();
    let store = open(dir.path());
    assert_eq!(store.user_version().unwrap(), 8);
}

#[test]
fn upsert_then_read_round_trips() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = open(dir.path());
    let file = todo();
    let t = task(1);

    store
        .upsert_fingerprint(&file, t, &fp("buy milk", 0), 1_000)
        .unwrap();

    let live = store.live_fingerprints(&file).unwrap();
    assert_eq!(live.len(), 1);
    assert_eq!(live[0].task, t);
    assert_eq!(live[0].fingerprint, fp("buy milk", 0));
    assert_eq!(live[0].updated_at_ms, 1_000);
    assert!(store.tombstoned_fingerprints(&file).unwrap().is_empty());
}

#[test]
fn upsert_is_idempotent_and_replaces() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = open(dir.path());
    let file = todo();
    let t = task(1);

    store
        .upsert_fingerprint(&file, t, &fp("buy milk", 0), 1_000)
        .unwrap();
    store
        .upsert_fingerprint(&file, t, &fp("buy oat milk", 2), 2_000)
        .unwrap();

    let live = store.live_fingerprints(&file).unwrap();
    assert_eq!(live.len(), 1, "an upsert replaces, it doesn't add a row");
    assert_eq!(live[0].fingerprint, fp("buy oat milk", 2));
    assert_eq!(live[0].updated_at_ms, 2_000);
}

#[test]
fn retire_moves_a_row_from_live_to_tombstoned() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = open(dir.path());
    let file = todo();
    let t = task(1);

    store
        .upsert_fingerprint(&file, t, &fp("buy milk", 0), 1_000)
        .unwrap();
    store.retire_fingerprint(&file, t, 3_000).unwrap();

    assert!(store.live_fingerprints(&file).unwrap().is_empty());
    let tombstoned = store.tombstoned_fingerprints(&file).unwrap();
    assert_eq!(
        tombstoned.len(),
        1,
        "retiring keeps the row, doesn't delete it"
    );
    assert_eq!(tombstoned[0].task, t);
}

#[test]
fn retire_is_idempotent() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = open(dir.path());
    let file = todo();
    let t = task(1);

    store
        .upsert_fingerprint(&file, t, &fp("buy milk", 0), 1_000)
        .unwrap();
    store.retire_fingerprint(&file, t, 3_000).unwrap();
    store.retire_fingerprint(&file, t, 4_000).unwrap();

    let tombstoned = store.tombstoned_fingerprints(&file).unwrap();
    assert_eq!(tombstoned.len(), 1);
}

#[test]
fn upsert_after_retire_revives_as_live() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = open(dir.path());
    let file = todo();
    let t = task(1);

    store
        .upsert_fingerprint(&file, t, &fp("buy milk", 0), 1_000)
        .unwrap();
    store.retire_fingerprint(&file, t, 3_000).unwrap();
    store
        .upsert_fingerprint(&file, t, &fp("buy milk", 0), 5_000)
        .unwrap();

    assert_eq!(store.live_fingerprints(&file).unwrap().len(), 1);
    assert!(store.tombstoned_fingerprints(&file).unwrap().is_empty());
}

#[test]
fn distinct_files_do_not_see_each_others_fingerprints() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = open(dir.path());
    let a = todo();
    let b = FilePath::new("other.txt").unwrap();
    let t = task(1);

    store
        .upsert_fingerprint(&a, t, &fp("buy milk", 0), 1_000)
        .unwrap();

    assert!(store.live_fingerprints(&b).unwrap().is_empty());
}

#[test]
fn a_fingerprint_with_no_creation_date_round_trips_as_none() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = open(dir.path());
    let file = todo();
    let t = task(1);
    let mut undated = fp("someday maybe", 0);
    undated.creation_date = None;

    store.upsert_fingerprint(&file, t, &undated, 1_000).unwrap();

    let live = store.live_fingerprints(&file).unwrap();
    assert_eq!(live[0].fingerprint.creation_date, None);
}

#[test]
fn retain_keeps_the_named_tasks_and_retires_the_rest() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = open(dir.path());
    let file = todo();
    let other = FilePath::new("other.txt").unwrap();
    for (n, desc) in [(1, "buy milk"), (2, "walk dog"), (3, "call mum")] {
        store
            .upsert_fingerprint(&file, task(n), &fp(desc, 0), 1_000)
            .unwrap();
    }
    store
        .upsert_fingerprint(&other, task(9), &fp("elsewhere", 0), 1_000)
        .unwrap();

    let retired = store
        .retain_live_fingerprints(&file, &BTreeSet::from([task(1)]), 2_000)
        .unwrap();

    assert_eq!(retired, 2);
    let live: Vec<TaskId> = store
        .live_fingerprints(&file)
        .unwrap()
        .into_iter()
        .map(|r| r.task)
        .collect();
    assert_eq!(live, vec![task(1)]);
    let tombstoned = store.tombstoned_fingerprints(&file).unwrap();
    assert_eq!(tombstoned.len(), 2);
    assert_eq!(
        tombstoned[0].fingerprint.description_norm, "walk dog",
        "a retired row keeps what it last looked like"
    );
    assert_eq!(store.live_fingerprints(&other).unwrap().len(), 1);
}

/// tasks/sync-drift line 1: a commit's fingerprints are the file's whole live set, so the row of
/// a task that left the file is retired in that same commit instead of staying live forever.
#[test]
fn a_commit_retires_the_rows_its_fingerprints_leave_out() {
    use txtodo_store::{CommitExtras, FingerprintRow, Projection};
    let dir = tempfile::tempdir().unwrap();
    let mut store = open(dir.path());
    let file = todo();
    let row = |n: u128, desc: &str, i: usize, at: u64| FingerprintRow {
        file: todo(),
        task: task(n),
        fingerprint: fp(desc, i),
        updated_at_ms: at,
    };
    let commit = |store: &mut Store, rows: Vec<FingerprintRow>| {
        let projection = Projection {
            file: todo(),
            bytes: b"whatever\n".to_vec(),
            hash: [1; 32],
            written_at_ms: 1,
        };
        let extras = CommitExtras {
            fingerprints: rows,
            ..CommitExtras::default()
        };
        store
            .commit_change_with(&[], &projection, None, &extras)
            .unwrap();
    };

    commit(
        &mut store,
        vec![row(1, "buy milk", 0, 1_000), row(2, "walk dog", 1, 1_000)],
    );
    commit(&mut store, vec![row(2, "walk dog", 0, 2_000)]);

    let live = store.live_fingerprints(&file).unwrap();
    assert_eq!(live.len(), 1);
    assert_eq!(live[0].task, task(2));
    let tombstoned = store.tombstoned_fingerprints(&file).unwrap();
    assert_eq!(tombstoned.len(), 1);
    assert_eq!(tombstoned[0].task, task(1));

    commit(&mut store, Vec::new());
    assert_eq!(
        store.live_fingerprints(&file).unwrap().len(),
        1,
        "an empty set (tagged mode) retires nothing"
    );
}
