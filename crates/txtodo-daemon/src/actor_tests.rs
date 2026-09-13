//! FileActor in-process with a FakeClock: adoption, apply, external change, own-write recognition,
//! crash recovery (interrupted rename), a stale client, undo and checkout.

use crate::actor::{ActorConfig, FileActor, SharedStore, hash_of};
use crate::clock::FakeClock;
use crate::handle::ActorError;
use crate::mutation::{Mutation, MutationError, TaskRef};
use crate::stats::Stats;
use std::path::Path;
use std::sync::{Arc, Mutex};
use txtodo_model::{DeviceId, FilePath, IdentityMode, Principal, Ulid};
use txtodo_store::{Seq, Store};

const A: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAA";

fn device() -> DeviceId {
    DeviceId::new(Ulid::from_u128(7))
}

fn user() -> Principal {
    Principal::User { device: device() }
}

fn store(dir: &Path) -> SharedStore {
    Arc::new(Mutex::new(
        Store::open(&dir.join("oplog.db")).unwrap_or_else(|e| panic!("open store: {e}")),
    ))
}

fn cfg(dir: &Path) -> ActorConfig {
    ActorConfig {
        path: FilePath::new("todo.txt").unwrap_or_else(|e| panic!("{e}")),
        disk: dir.join("todo.txt"),
        device: device(),
        stats: Arc::new(Stats::default()),
        identity_mode: IdentityMode::Tagged,
        tree_dirty: Arc::new(crate::tree_dirty::TreeDirty::default()),
    }
}

fn open(dir: &Path, store: &SharedStore, clock: &Arc<FakeClock>) -> FileActor {
    let clock: Arc<dyn crate::clock::Clock> = clock.clone();
    FileActor::open(cfg(dir), Arc::clone(store), clock)
        .unwrap_or_else(|e| panic!("open actor: {e}"))
}

fn disk(dir: &Path) -> String {
    String::from_utf8(std::fs::read(dir.join("todo.txt")).unwrap_or_default()).unwrap_or_default()
}

fn last_seq(store: &SharedStore) -> Option<Seq> {
    store
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .last_seq()
        .unwrap_or(None)
}

#[tokio::test]
async fn adoption_stamps_ids_and_records_external_inserts() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("todo.txt"),
        format!("(A) buy ducks id:{A}\nwalk the dog\n"),
    )
    .unwrap();
    let store = store(dir.path());
    let clock = Arc::new(FakeClock::new(1_000));
    let handle = open(dir.path(), &store, &clock).spawn();
    let text = disk(dir.path());
    assert!(
        text.starts_with(&format!("(A) buy ducks id:{A}\nwalk the dog id:")),
        "{text}"
    );
    let got = handle.get().await.unwrap();
    assert_eq!(got.bytes, text.as_bytes());
    assert_eq!(got.hash, hash_of(text.as_bytes()));
    let newest = store
        .lock()
        .unwrap()
        .newest(&FilePath::new("todo.txt").unwrap(), 10)
        .unwrap();
    assert_eq!(newest.len(), 2);
    assert!(
        newest
            .iter()
            .all(|s| matches!(s.op.principal, Principal::External { .. }))
    );
}

#[tokio::test]
async fn apply_writes_the_file_records_user_ops_and_notifies_subscribers() {
    let dir = tempfile::tempdir().unwrap();
    let store = store(dir.path());
    let clock = Arc::new(FakeClock::new(1_000));
    let handle = open(dir.path(), &store, &clock).spawn();
    let mut changes = handle.subscribe().await.unwrap();
    let applied = handle
        .apply(
            vec![Mutation::Add {
                line: "(B) call mum @phone".into(),
            }],
            user(),
        )
        .await
        .unwrap();
    assert_eq!(applied.applied, 1);
    let text = disk(dir.path());
    assert!(
        text.starts_with("(B) call mum @phone id:") && text.ends_with('\n'),
        "{text}"
    );
    assert_eq!(applied.hash, hash_of(text.as_bytes()));
    let change = changes.recv().await.unwrap();
    assert_eq!(change.hash, applied.hash);
    assert!(matches!(change.ops[0].op.principal, Principal::User { .. }));
    assert_eq!(change.ops[0].seq, Seq(1));
    let stale = TaskRef {
        line_number: 1,
        task_id: Some(crate::state::task_id(1)),
    };
    let err = handle
        .apply(
            vec![Mutation::Delete {
                task: stale,
                leave_blank: false,
            }],
            user(),
        )
        .await
        .unwrap_err();
    assert!(
        matches!(err, ActorError::Mutation(MutationError::Stale { .. })),
        "{err}"
    );
}

#[tokio::test]
async fn external_edits_reconcile_and_own_writes_are_ignored() {
    let dir = tempfile::tempdir().unwrap();
    let store = store(dir.path());
    let clock = Arc::new(FakeClock::new(1_000));
    let handle = open(dir.path(), &store, &clock).spawn();
    handle
        .apply(
            vec![Mutation::Add {
                line: "(B) call mum @phone".into(),
            }],
            user(),
        )
        .await
        .unwrap();
    handle.external_change().await.unwrap();
    handle.get().await.unwrap();
    assert_eq!(
        last_seq(&store),
        Some(Seq(1)),
        "our own rename derives nothing"
    );
    let text = disk(dir.path()).replace("call mum", "call dad") + "new one\n";
    std::fs::write(dir.path().join("todo.txt"), &text).unwrap();
    handle.external_change().await.unwrap();
    let got = handle.get().await.unwrap();
    let after = disk(dir.path());
    assert_eq!(got.bytes, after.as_bytes(), "state matches disk");
    assert!(
        after.contains("call dad")
            && after
                .lines()
                .nth(1)
                .is_some_and(|l| l.starts_with("new one id:")),
        "{after}"
    );
    assert_eq!(last_seq(&store), Some(Seq(3)), "edit_text + insert");
}

#[tokio::test]
async fn an_interrupted_rename_is_completed_on_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let store = store(dir.path());
    let clock = Arc::new(FakeClock::new(1_000));
    let handle = open(dir.path(), &store, &clock).spawn();
    handle
        .apply(
            vec![Mutation::Add {
                line: "first".into(),
            }],
            user(),
        )
        .await
        .unwrap();
    let before = disk(dir.path());
    handle
        .apply(
            vec![Mutation::Add {
                line: "second".into(),
            }],
            user(),
        )
        .await
        .unwrap();
    let after = disk(dir.path());
    drop(handle);
    std::fs::write(dir.path().join("todo.txt"), &before).unwrap();
    let reopened = open(dir.path(), &store, &clock);
    assert_eq!(
        reopened.writes_total(),
        1,
        "exactly one write completed the interrupted rename"
    );
    assert_eq!(disk(dir.path()), after);
    assert_eq!(last_seq(&store), Some(Seq(2)), "no new ops were derived");
}

#[tokio::test]
async fn undo_restores_bytes_exactly_and_checkout_renders_the_past() {
    let dir = tempfile::tempdir().unwrap();
    let store = store(dir.path());
    let clock = Arc::new(FakeClock::new(1_000));
    let handle = open(dir.path(), &store, &clock).spawn();
    handle
        .apply(
            vec![Mutation::Add {
                line: "(A) first".into(),
            }],
            user(),
        )
        .await
        .unwrap();
    let one = disk(dir.path());
    clock.advance_ms(1_000);
    handle
        .apply(
            vec![Mutation::Add {
                line: "second".into(),
            }],
            user(),
        )
        .await
        .unwrap();
    let two = disk(dir.path());
    clock.advance_ms(1_000);
    let external = two.replace("(A) first", "(B) first!");
    std::fs::write(dir.path().join("todo.txt"), &external).unwrap();
    handle.external_change().await.unwrap();
    assert_eq!(disk(dir.path()), external);
    let undone = handle.undo(2, user()).await.unwrap();
    assert_eq!(undone.applied, 2, "priority + text edit inverted");
    assert_eq!(
        disk(dir.path()),
        two,
        "undo restores the previous bytes exactly"
    );
    assert_eq!(
        handle.checkout(1_500).await.unwrap(),
        one.as_bytes(),
        "between the two adds"
    );
    assert_eq!(
        handle.checkout(2_000).await.unwrap(),
        two.as_bytes(),
        "inclusive at the second add"
    );
    assert!(
        handle.checkout(10).await.unwrap().is_empty(),
        "before anything"
    );
}
