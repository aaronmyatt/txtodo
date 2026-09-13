//! Sidecar identity fingerprints: upsert → live, retire → tombstoned (idempotent, kept not
//! deleted), a later upsert revives a tombstoned row, files don't see each other's rows, and the
//! schema lands at 5.
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
    assert_eq!(store.user_version().unwrap(), 6);
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
    let b = FilePath::new("done.txt").unwrap();
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
