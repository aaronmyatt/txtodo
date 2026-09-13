//! Two actors that share a Loro lineage (B was seeded from A's mirror snapshot, as pairing will
//! do) edit the same word offline; B imports A's updates: exactly one flag, the merged line on
//! disk, resolve merged writes no op, resolve mine writes one edit and clears the flag, a second
//! resolve is refused. Different words merge with no flag.

use crate::actor::{ActorConfig, FileActor, SharedStore};
use crate::clock::{Clock, FakeClock};
use crate::handle::{ActorError, ActorHandle, Resolution};
use crate::mutation::{Mutation, TaskRef};
use crate::stats::Stats;
use std::path::Path;
use std::sync::{Arc, Mutex};
use txtodo_model::{DeviceId, FilePath, IdentityMode, Principal, TaskId, Ulid};
use txtodo_store::{Projection, Seq, Store};

const T: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAT";

fn dev(n: u128) -> DeviceId {
    DeviceId::new(Ulid::from_u128(n))
}

fn store(dir: &Path) -> SharedStore {
    Arc::new(Mutex::new(
        Store::open(&dir.join("oplog.db")).unwrap_or_else(|e| panic!("open store: {e}")),
    ))
}

fn cfg(dir: &Path, device: DeviceId) -> ActorConfig {
    ActorConfig {
        path: FilePath::new("todo.txt").unwrap_or_else(|e| panic!("{e}")),
        disk: dir.join("todo.txt"),
        device,
        stats: Arc::new(Stats::default()),
        identity_mode: IdentityMode::Tagged,
        tree_dirty: Arc::new(crate::tree_dirty::TreeDirty::default()),
    }
}

fn open(dir: &Path, device: DeviceId, store: &SharedStore, clock: &Arc<FakeClock>) -> FileActor {
    let clock: Arc<dyn Clock> = clock.clone();
    FileActor::open(cfg(dir, device), Arc::clone(store), clock)
        .unwrap_or_else(|e| panic!("open actor: {e}"))
}

fn disk(dir: &Path) -> String {
    String::from_utf8(std::fs::read(dir.join("todo.txt")).unwrap_or_default()).unwrap_or_default()
}

fn edit(line: u32, new_line: &str) -> Vec<Mutation> {
    vec![Mutation::Edit {
        task: TaskRef {
            line_number: line as usize,
            task_id: None,
        },
        new_line: new_line.to_owned(),
    }]
}

/// A with one task on disk, and B seeded from A: same bytes, A's projection hash, A's mirror
/// snapshot at seq 0 — what pairing ships as "snapshot plus ops".
async fn paired() -> (
    tempfile::TempDir,
    ActorHandle,
    tempfile::TempDir,
    ActorHandle,
) {
    let a_dir = tempfile::tempdir().unwrap();
    let line = format!("buy ducks +farm id:{T}\n");
    std::fs::write(a_dir.path().join("todo.txt"), &line).unwrap();
    let clock = Arc::new(FakeClock::new(1_000));
    let a_store = store(a_dir.path());
    let a = open(a_dir.path(), dev(1), &a_store, &clock);
    let snapshot = a.mirror.snapshot().unwrap();
    let hash = a.hash;
    let a = a.spawn();
    let b_dir = tempfile::tempdir().unwrap();
    std::fs::write(b_dir.path().join("todo.txt"), &line).unwrap();
    let b_store = store(b_dir.path());
    {
        let mut s = b_store.lock().unwrap();
        s.put_projection(&Projection {
            file: FilePath::new("todo.txt").unwrap(),
            bytes: line.clone().into_bytes(),
            hash,
            written_at_ms: 1_000,
        })
        .unwrap();
        s.put_mirror(&FilePath::new("todo.txt").unwrap(), &snapshot, Seq(0))
            .unwrap();
    }
    let b = open(b_dir.path(), dev(2), &b_store, &clock).spawn();
    (a_dir, a, b_dir, b)
}

async fn sync_a_into_b(a: &ActorHandle, b: &ActorHandle) -> u32 {
    let since = b.version().await.unwrap();
    let updates = a.export_since(since).await.unwrap();
    b.import_updates(updates, dev(1)).await.unwrap().applied
}

#[tokio::test]
async fn same_word_on_both_sides_raises_one_flag_and_resolve_clears_it() {
    let (_ad, a, bd, b) = paired().await;
    let user = |d: u128| Principal::User { device: dev(d) };
    a.apply(edit(1, &format!("buy geese +farm id:{T}")), user(1))
        .await
        .unwrap();
    b.apply(edit(1, &format!("buy cows +farm id:{T}")), user(2))
        .await
        .unwrap();
    let applied = sync_a_into_b(&a, &b).await;
    assert!(applied >= 1, "the merge landed as ops on B: {applied}");
    let flags = b.conflicts().await.unwrap();
    assert_eq!(flags.len(), 1, "{flags:?}");
    assert_eq!(flags[0].row.task, TaskId::new(Ulid::parse(T).unwrap()));
    assert_eq!(flags[0].line_number, 1);
    assert_eq!(
        flags[0].row.mine,
        format!("buy cows +farm id:{T}").into_bytes()
    );
    assert_eq!(
        flags[0].row.theirs,
        format!("buy geese +farm id:{T}").into_bytes()
    );
    let merged = disk(bd.path());
    assert!(
        merged.contains(&format!(" id:{T}")),
        "still one task: {merged}"
    );
    assert_eq!(merged.lines().count(), 1);
    // merged: keep what is in the file, write no op, clear the flag
    let r = b
        .resolve(
            TaskRef {
                line_number: 1,
                task_id: None,
            },
            Resolution::Merged,
            user(2),
        )
        .await
        .unwrap();
    assert_eq!(r.applied, 0);
    assert!(b.conflicts().await.unwrap().is_empty());
    assert_eq!(disk(bd.path()), merged, "merged changes nothing on disk");
    // a second resolve has nothing to resolve
    let again = b
        .resolve(
            TaskRef {
                line_number: 1,
                task_id: None,
            },
            Resolution::Mine,
            user(2),
        )
        .await;
    assert!(matches!(again, Err(ActorError::NoFlag(_))), "{again:?}");
}

#[tokio::test]
async fn resolve_mine_writes_one_edit_back_and_different_words_never_flag() {
    let (_ad, a, bd, b) = paired().await;
    let user = |d: u128| Principal::User { device: dev(d) };
    a.apply(edit(1, &format!("buy geese +farm id:{T}")), user(1))
        .await
        .unwrap();
    b.apply(edit(1, &format!("buy cows +farm id:{T}")), user(2))
        .await
        .unwrap();
    sync_a_into_b(&a, &b).await;
    assert_eq!(b.conflicts().await.unwrap().len(), 1);
    let r = b
        .resolve(
            TaskRef {
                line_number: 1,
                task_id: None,
            },
            Resolution::Mine,
            user(2),
        )
        .await
        .unwrap();
    assert!(r.applied >= 1, "one edit written back: {}", r.applied);
    assert_eq!(disk(bd.path()), format!("buy cows +farm id:{T}\n"));
    assert!(b.conflicts().await.unwrap().is_empty());
    // Different words from here on: A changes the project, B the verb — no flag.
    a.apply(edit(1, &format!("buy geese +barn id:{T}")), user(1))
        .await
        .unwrap();
    let before = b.version().await.unwrap();
    sync_a_into_b(&a, &b).await;
    assert!(
        b.conflicts().await.unwrap().is_empty(),
        "sequential, not concurrent"
    );
    assert_ne!(b.version().await.unwrap(), before);
}
