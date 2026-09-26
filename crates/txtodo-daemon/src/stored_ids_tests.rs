//! tasks/sync-drift line 1: a task that leaves a sidecar file loses its live fingerprint row in the
//! same commit, so a restart lines every row up with a line and mints nothing; and a store that
//! already piled up stale live rows is repaired by position on the next start, not re-minted.

use crate::actor::{ActorConfig, FileActor, SharedStore};
use crate::clock::FakeClock;
use crate::mutation::{Mutation, TaskRef};
use crate::stats::Stats;
use std::path::Path;
use std::sync::{Arc, Mutex};
use txtodo_model::{DeviceId, FilePath, IdentityMode, Principal, TaskId, Ulid};
use txtodo_store::{FingerprintRow, Store};

fn device() -> DeviceId {
    DeviceId::new(Ulid::from_u128(7))
}

fn path() -> FilePath {
    FilePath::new("todo.txt").unwrap_or_else(|e| panic!("{e}"))
}

fn store(dir: &Path) -> SharedStore {
    Arc::new(Mutex::new(
        Store::open(&dir.join("oplog.db")).unwrap_or_else(|e| panic!("open store: {e}")),
    ))
}

fn open(dir: &Path, store: &SharedStore, clock: &Arc<FakeClock>) -> FileActor {
    let cfg = ActorConfig {
        path: path(),
        disk: dir.join("todo.txt"),
        device: device(),
        stats: Arc::new(Stats::default()),
        identity_mode: IdentityMode::Sidecar,
        tree_dirty: Arc::new(crate::tree_dirty::TreeDirty::default()),
        layout: crate::layout_state::SharedLayout::default(),
    };
    let clock: Arc<dyn crate::clock::Clock> = clock.clone();
    FileActor::open(cfg, Arc::clone(store), clock).unwrap_or_else(|e| panic!("open actor: {e}"))
}

fn locked(store: &SharedStore) -> std::sync::MutexGuard<'_, Store> {
    store
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn live(store: &SharedStore) -> Vec<FingerprintRow> {
    locked(store).live_fingerprints(&path()).unwrap_or_default()
}

fn tombstoned(store: &SharedStore) -> usize {
    locked(store)
        .tombstoned_fingerprints(&path())
        .unwrap_or_default()
        .len()
}

fn op_count(store: &SharedStore) -> usize {
    locked(store)
        .newest(&path(), 1_000)
        .unwrap_or_default()
        .len()
}

fn seed(dir: &Path, text: &str) {
    std::fs::write(dir.join("todo.txt"), text).unwrap_or_else(|e| panic!("seed: {e}"));
}

fn ids(contents: &crate::contents::Contents) -> Vec<TaskId> {
    contents.task_ids.iter().flatten().copied().collect()
}

#[tokio::test]
async fn a_delete_then_restart_reuses_every_id_and_mints_nothing() {
    let dir = tempfile::tempdir().unwrap();
    seed(
        dir.path(),
        "buy ducks +farm\n\nwalk the dog @home\ncall mum @phone\n",
    );
    let store = store(dir.path());
    let clock = Arc::new(FakeClock::new(1_000));
    let first = open(dir.path(), &store, &clock).spawn();
    let before = first.get().await.unwrap();
    let walk = before.task_ids[2];
    first
        .apply(
            vec![Mutation::Delete {
                task: TaskRef {
                    line_number: 3,
                    task_id: walk,
                },
                leave_blank: false,
            }],
            Principal::User { device: device() },
        )
        .await
        .unwrap();
    let kept = ids(&first.get().await.unwrap());
    assert_eq!(live(&store).len(), 2, "retired by the delete's own commit");
    let ops = op_count(&store);
    drop(first);

    let second = open(dir.path(), &store, &clock).spawn();

    assert_eq!(op_count(&store), ops, "reopening reconciles nothing");
    assert_eq!(
        ids(&second.get().await.unwrap()),
        kept,
        "same ids, none minted"
    );
    let rows: Vec<TaskId> = live(&store).into_iter().map(|r| r.task).collect();
    assert_eq!(rows, kept, "one live row per line");
    assert_eq!(
        tombstoned(&store),
        1,
        "the deleted task's row is kept, retired"
    );
}

#[tokio::test]
async fn an_editor_delete_retires_the_row_too() {
    let dir = tempfile::tempdir().unwrap();
    seed(
        dir.path(),
        "buy ducks +farm\nwalk the dog @home\ncall mum @phone\n",
    );
    let store = store(dir.path());
    let clock = Arc::new(FakeClock::new(1_000));
    let mut actor = open(dir.path(), &store, &clock);
    seed(dir.path(), "buy ducks +farm\ncall mum @phone\n");
    clock.advance_ms(5_000);

    actor.on_external_change().unwrap();

    assert_eq!(live(&store).len(), 2);
    assert_eq!(tombstoned(&store), 1);
}

/// The shape found in the wild: a past re-mint left a full second set of rows at the same
/// positions with the same text, one step older, plus a row for a line long gone.
#[tokio::test]
async fn a_restart_repairs_stale_rows_by_position_instead_of_re_minting() {
    let dir = tempfile::tempdir().unwrap();
    seed(
        dir.path(),
        "buy ducks +farm\n\nwalk the dog @home\ncall mum @phone\n",
    );
    let store = store(dir.path());
    let clock = Arc::new(FakeClock::new(1_000));
    let first = open(dir.path(), &store, &clock).spawn();
    let kept = ids(&first.get().await.unwrap());
    drop(first);
    let current = live(&store);
    {
        let mut s = locked(&store);
        for (n, row) in (100..).zip(&current) {
            let stale = TaskId::new(Ulid::from_u128(n));
            s.upsert_fingerprint(&path(), stale, &row.fingerprint, 900)
                .unwrap();
        }
        let mut gone = current[0].fingerprint.clone();
        gone.line_index = 3;
        gone.description_norm = "long gone".to_owned();
        let gone_id = TaskId::new(Ulid::from_u128(200));
        s.upsert_fingerprint(&path(), gone_id, &gone, 500).unwrap();
    }
    let ops = op_count(&store);

    let second = open(dir.path(), &store, &clock).spawn();

    assert_eq!(op_count(&store), ops, "repaired, not re-minted");
    assert_eq!(ids(&second.get().await.unwrap()), kept);
    let rows: Vec<TaskId> = live(&store).into_iter().map(|r| r.task).collect();
    assert_eq!(rows, kept);
    assert_eq!(tombstoned(&store), current.len() + 1);
}

/// A tie the repair can't break (two newest rows with one line's exact fingerprint) is left to
/// the old path: re-mint, rather than guess which id a peer knows the line by.
#[tokio::test]
async fn an_ambiguous_owner_falls_back_to_re_minting() {
    let dir = tempfile::tempdir().unwrap();
    seed(dir.path(), "buy ducks +farm\nwalk the dog @home\n");
    let store = store(dir.path());
    let clock = Arc::new(FakeClock::new(1_000));
    drop(open(dir.path(), &store, &clock).spawn());
    let current = live(&store);
    {
        let twin = TaskId::new(Ulid::from_u128(100));
        let row = &current[0];
        let mut s = locked(&store);
        s.upsert_fingerprint(&path(), twin, &row.fingerprint, row.updated_at_ms)
            .unwrap();
    }
    let ops = op_count(&store);
    clock.advance_ms(5_000);

    drop(open(dir.path(), &store, &clock).spawn());

    assert!(
        op_count(&store) > ops,
        "the old path: every line minted again"
    );
    assert_eq!(
        live(&store).len(),
        2,
        "that commit's set is the whole live set"
    );
}

/// A sidecar file whose last task left keeps that row (an empty set retires nothing); the next
/// start's repair has no line to give it and retires it.
#[tokio::test]
async fn a_file_emptied_of_tasks_is_cleaned_up_on_the_next_start() {
    let dir = tempfile::tempdir().unwrap();
    seed(dir.path(), "buy ducks +farm\n");
    let store = store(dir.path());
    let clock = Arc::new(FakeClock::new(1_000));
    let mut actor = open(dir.path(), &store, &clock);
    seed(dir.path(), "");
    clock.advance_ms(5_000);
    actor.on_external_change().unwrap();
    assert_eq!(live(&store).len(), 1, "an empty commit retires nothing");
    drop(actor);
    let ops = op_count(&store);

    drop(open(dir.path(), &store, &clock));

    assert!(live(&store).is_empty());
    assert_eq!(tombstoned(&store), 1);
    assert_eq!(op_count(&store), ops);
}
