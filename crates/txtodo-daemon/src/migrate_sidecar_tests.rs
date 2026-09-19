//! `FileActor::on_migrate_to_sidecar` (tasks/sidecar-migrate-tagged): a Tagged document loses its
//! `id:` tags through ordinary ops, keeps every task's identity as fingerprint rows, and behaves
//! as a Sidecar document from then on — including across a restart.

use crate::actor::{ActorConfig, FileActor, SharedStore};
use crate::clock::FakeClock;
use crate::migrate_sidecar::Migrated;
use crate::mutation::Mutation;
use crate::stats::Stats;
use std::path::Path;
use std::sync::{Arc, Mutex};
use txtodo_model::{DeviceId, FilePath, IdentityMode, OpKind, Principal, TaskId, Ulid};
use txtodo_store::Store;

const A: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAA";
const B: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAB";
const QUOTED: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAC";

fn device() -> DeviceId {
    DeviceId::new(Ulid::from_u128(7))
}

fn path() -> FilePath {
    FilePath::new("todo.txt").unwrap_or_else(|e| panic!("{e}"))
}

fn open(dir: &Path, store: &SharedStore, clock: &Arc<FakeClock>, mode: IdentityMode) -> FileActor {
    let cfg = ActorConfig {
        path: path(),
        disk: dir.join("todo.txt"),
        device: device(),
        stats: Arc::new(Stats::default()),
        identity_mode: mode,
        tree_dirty: Arc::new(crate::tree_dirty::TreeDirty::default()),
    };
    let clock: Arc<dyn crate::clock::Clock> = clock.clone();
    FileActor::open(cfg, Arc::clone(store), clock).unwrap_or_else(|e| panic!("open actor: {e}"))
}

fn store(dir: &Path) -> SharedStore {
    Arc::new(Mutex::new(
        Store::open(&dir.join("oplog.db")).unwrap_or_else(|e| panic!("open store: {e}")),
    ))
}

fn disk(dir: &Path) -> String {
    String::from_utf8(std::fs::read(dir.join("todo.txt")).unwrap_or_default()).unwrap_or_default()
}

fn ulid(s: &str) -> TaskId {
    TaskId::new(Ulid::parse(s).unwrap_or_else(|| panic!("bad ulid {s}")))
}

fn fingerprint_tasks(store: &SharedStore) -> Vec<TaskId> {
    store
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .live_fingerprints(&path())
        .unwrap_or_default()
        .into_iter()
        .map(|r| r.task)
        .collect()
}

fn op_count(store: &SharedStore) -> usize {
    store
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .newest(&path(), 1_000)
        .unwrap_or_default()
        .len()
}

fn seed(dir: &Path) {
    std::fs::write(
        dir.join("todo.txt"),
        format!(
            "(A) buy ducks id:{A}\n\nx 2026-09-19 2026-09-18 done (id:{QUOTED}) id:{B} pri:B\n"
        ),
    )
    .unwrap_or_else(|e| panic!("seed: {e}"));
}

#[tokio::test]
async fn dry_run_counts_and_touches_nothing() {
    let dir = tempfile::tempdir().unwrap();
    seed(dir.path());
    let store = store(dir.path());
    let clock = Arc::new(FakeClock::new(1_000));
    let handle = open(dir.path(), &store, &clock, IdentityMode::Tagged).spawn();
    let before = disk(dir.path());
    let ops_before = op_count(&store);

    let got = handle.migrate_to_sidecar(true).await.unwrap();

    assert_eq!(
        got,
        Migrated {
            tasks: 2,
            stripped: 2,
            renumbered: 0
        }
    );
    assert_eq!(disk(dir.path()), before);
    assert_eq!(op_count(&store), ops_before);
    assert!(fingerprint_tasks(&store).is_empty());
}

#[tokio::test]
async fn strips_own_tags_and_keeps_every_identity() {
    let dir = tempfile::tempdir().unwrap();
    seed(dir.path());
    let store = store(dir.path());
    let clock = Arc::new(FakeClock::new(1_000));
    let handle = open(dir.path(), &store, &clock, IdentityMode::Tagged).spawn();
    let ops_before = op_count(&store);

    let got = handle.migrate_to_sidecar(false).await.unwrap();

    assert_eq!(got.stripped, 2);
    assert_eq!(
        disk(dir.path()),
        format!("(A) buy ducks\n\nx 2026-09-19 2026-09-18 done (id:{QUOTED}) pri:B\n")
    );
    assert_eq!(fingerprint_tasks(&store), vec![ulid(A), ulid(B)]);
    let newest = store.lock().unwrap().newest(&path(), 1_000).unwrap();
    let migration_ops = &newest[..newest.len() - ops_before];
    assert!(!migration_ops.is_empty());
    for stored in migration_ops {
        assert!(matches!(stored.op.principal, Principal::User { .. }));
        let OpKind::EditText { task, .. } = &stored.op.kind else {
            panic!("a migration op is a plain text edit: {:?}", stored.op.kind);
        };
        assert!([ulid(A), ulid(B)].contains(task));
    }
}

#[tokio::test]
async fn is_idempotent() {
    let dir = tempfile::tempdir().unwrap();
    seed(dir.path());
    let store = store(dir.path());
    let clock = Arc::new(FakeClock::new(1_000));
    let handle = open(dir.path(), &store, &clock, IdentityMode::Tagged).spawn();
    handle.migrate_to_sidecar(false).await.unwrap();
    let after_first = disk(dir.path());
    let ops = op_count(&store);

    let again = handle.migrate_to_sidecar(false).await.unwrap();

    assert_eq!(again.stripped, 0);
    assert_eq!(disk(dir.path()), after_first);
    assert_eq!(op_count(&store), ops);
}

#[tokio::test]
async fn a_task_added_afterwards_gets_no_tag() {
    let dir = tempfile::tempdir().unwrap();
    seed(dir.path());
    let store = store(dir.path());
    let clock = Arc::new(FakeClock::new(1_000));
    let handle = open(dir.path(), &store, &clock, IdentityMode::Tagged).spawn();
    handle.migrate_to_sidecar(false).await.unwrap();

    handle
        .apply(
            vec![Mutation::Add {
                line: "2026-09-19 call mum".into(),
            }],
            Principal::User { device: device() },
        )
        .await
        .unwrap();

    let text = disk(dir.path());
    assert!(text.ends_with("call mum\n"), "{text}");
    assert!(!text.contains(&format!("id:{A}")) && !text.contains(&format!("id:{B}")));
    assert_eq!(fingerprint_tasks(&store).len(), 3);
}

#[tokio::test]
async fn a_restart_in_sidecar_mode_keeps_the_same_ids_and_mints_nothing() {
    let dir = tempfile::tempdir().unwrap();
    seed(dir.path());
    let store = store(dir.path());
    let clock = Arc::new(FakeClock::new(1_000));
    let first = open(dir.path(), &store, &clock, IdentityMode::Tagged).spawn();
    first.migrate_to_sidecar(false).await.unwrap();
    let text = disk(dir.path());
    let ops = op_count(&store);
    drop(first);

    let second = open(dir.path(), &store, &clock, IdentityMode::Sidecar).spawn();

    assert_eq!(disk(dir.path()), text);
    assert_eq!(op_count(&store), ops, "reopening reconciles nothing");
    assert_eq!(fingerprint_tasks(&store), vec![ulid(A), ulid(B)]);
    let contents = second.get().await.unwrap();
    assert_eq!(contents.bytes, text.as_bytes());
}

#[tokio::test]
async fn a_tagged_document_opened_as_sidecar_keeps_its_ids_until_migrated() {
    let dir = tempfile::tempdir().unwrap();
    seed(dir.path());
    let store = store(dir.path());
    let clock = Arc::new(FakeClock::new(1_000));
    // The interrupted-migration shape: the document was adopted while Tagged, then the workspace
    // flipped to Sidecar and the process died before this document was migrated.
    drop(open(dir.path(), &store, &clock, IdentityMode::Tagged).spawn());
    let handle = open(dir.path(), &store, &clock, IdentityMode::Sidecar).spawn();
    assert!(disk(dir.path()).contains(&format!("id:{A}")));

    let got = handle.migrate_to_sidecar(false).await.unwrap();

    assert_eq!(got.stripped, 2);
    assert_eq!(fingerprint_tasks(&store), vec![ulid(A), ulid(B)]);
    assert!(!disk(dir.path()).contains(&format!("id:{A}")));
}

#[tokio::test]
async fn a_tagged_document_that_committed_under_sidecar_is_still_migrated() {
    let dir = tempfile::tempdir().unwrap();
    seed(dir.path());
    let store = store(dir.path());
    let clock = Arc::new(FakeClock::new(1_000));
    drop(open(dir.path(), &store, &clock, IdentityMode::Tagged).spawn());
    let handle = open(dir.path(), &store, &clock, IdentityMode::Sidecar).spawn();
    // Any commit under Sidecar lands fingerprint rows for every task, tags or no tags.
    handle
        .apply(
            vec![Mutation::Add {
                line: "2026-09-19 call mum".into(),
            }],
            Principal::User { device: device() },
        )
        .await
        .unwrap();
    assert!(disk(dir.path()).contains(&format!("id:{A}")));
    assert_eq!(fingerprint_tasks(&store).len(), 3);

    let dry = handle.migrate_to_sidecar(true).await.unwrap();
    let got = handle.migrate_to_sidecar(false).await.unwrap();

    assert_eq!((dry.stripped, got.stripped), (2, 2));
    let text = disk(dir.path());
    assert!(!text.contains(&format!("id:{A}")) && !text.contains(&format!("id:{B}")));
    assert!(text.ends_with("call mum\n"), "{text}");
}

#[tokio::test]
async fn the_loro_mirror_still_agrees_with_the_state() {
    let dir = tempfile::tempdir().unwrap();
    seed(dir.path());
    let store = store(dir.path());
    let clock = Arc::new(FakeClock::new(1_000));
    let mut actor = open(dir.path(), &store, &clock, IdentityMode::Tagged);

    actor.on_migrate_to_sidecar(false).unwrap();

    assert!(actor.mirror.agrees_with(&actor.state));
    assert_eq!(actor.state.mode(), IdentityMode::Sidecar);
}

#[tokio::test]
async fn a_repeated_id_keeps_the_first_line_and_renumbers_the_rest() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("todo.txt"),
        format!("first id:{A}\nother id:{B}\nsecond copy id:{A}\n"),
    )
    .unwrap();
    let store = store(dir.path());
    let clock = Arc::new(FakeClock::new(1_000));
    let handle = open(dir.path(), &store, &clock, IdentityMode::Tagged).spawn();

    let dry = handle.migrate_to_sidecar(true).await.unwrap();
    let got = handle.migrate_to_sidecar(false).await.unwrap();

    assert_eq!((dry.renumbered, got.renumbered, got.stripped), (1, 1, 3));
    assert_eq!(disk(dir.path()), "first\nother\nsecond copy\n");
    let rows = fingerprint_tasks(&store);
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[0], ulid(A), "the first occurrence keeps the id");
    assert_eq!(rows[1], ulid(B));
    assert_ne!(rows[2], ulid(A), "the repeat got a fresh one");
}

#[tokio::test]
async fn a_prose_mention_of_another_id_is_not_stripped_by_a_second_run() {
    let dir = tempfile::tempdir().unwrap();
    // The line's own tag comes first; the second `id:<ULID>` is prose that merely quotes an id.
    std::fs::write(
        dir.path().join("todo.txt"),
        format!("note id:{A} quotes id:{QUOTED}\n"),
    )
    .unwrap();
    let store = store(dir.path());
    let clock = Arc::new(FakeClock::new(1_000));
    let handle = open(dir.path(), &store, &clock, IdentityMode::Tagged).spawn();
    handle.migrate_to_sidecar(false).await.unwrap();
    assert_eq!(disk(dir.path()), format!("note quotes id:{QUOTED}\n"));

    let again = handle.migrate_to_sidecar(false).await.unwrap();

    assert_eq!(again.stripped, 0);
    assert_eq!(disk(dir.path()), format!("note quotes id:{QUOTED}\n"));
}
