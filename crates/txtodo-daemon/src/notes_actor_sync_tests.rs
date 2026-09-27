//! `NotesActor` against its op log (task notes-sync, moved out of `notes_actor.rs` for its line
//! budget): the disk seed at open, a peer's ops into a fresh actor, and (task notes-no-base) a
//! log with no base op that must neither block an import nor stay unrepaired.

use crate::actor::{SharedStore, hash_of};
use crate::clock::{Clock, FakeClock};
use crate::notes_actor::{NotesActor, NotesActorConfig};
use std::sync::{Arc, Mutex, PoisonError};
use txtodo_model::{DeviceId, FilePath, Hlc, Op, OpId, OpKind, Principal, TextEdit, Ulid};
use txtodo_store::{Projection, Seq, Store};

fn setup(dir: &std::path::Path, n: u128) -> (SharedStore, Arc<dyn Clock>, NotesActorConfig) {
    let store: SharedStore = Arc::new(Mutex::new(
        Store::open(&dir.join(format!("oplog-{n}.db"))).unwrap_or_else(|e| panic!("{e}")),
    ));
    let clock: Arc<dyn Clock> = Arc::new(FakeClock::new(1_000 + n as u64));
    let cfg = NotesActorConfig {
        path: FilePath::new("tasks/abc/notes.md").unwrap_or_else(|e| panic!("{e}")),
        disk: dir.join(format!("dev{n}/tasks/abc/notes.md")),
        device: DeviceId::new(Ulid::from_u128(n)),
    };
    std::fs::create_dir_all(cfg.disk.parent().unwrap_or(dir)).unwrap_or_else(|e| panic!("{e}"));
    (store, clock, cfg)
}

fn ops_for(store: &SharedStore, path: &FilePath) -> Vec<Op> {
    let guard = store.lock().unwrap_or_else(PoisonError::into_inner);
    guard
        .for_file(path, Seq(0))
        .unwrap_or_else(|e| panic!("{e}"))
        .into_iter()
        .map(|s| s.op)
        .collect()
}

#[test]
fn a_hand_written_notes_md_is_seeded_as_one_op_on_open_and_only_once() {
    let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    let (store, clock, cfg) = setup(dir.path(), 1);
    std::fs::write(&cfg.disk, "# Written by hand\n").unwrap_or_else(|e| panic!("{e}"));
    let actor = NotesActor::open(cfg.clone(), Arc::clone(&store), Arc::clone(&clock))
        .unwrap_or_else(|e| panic!("open: {e}"));
    assert_eq!(actor.contents().0, b"# Written by hand\n");
    let ops = ops_for(&store, &cfg.path);
    assert_eq!(ops.len(), 1, "one seed op for the peer to fetch");
    assert!(matches!(ops[0].principal, Principal::External { .. }));
    drop(actor);
    // A reopen with the file unchanged mints nothing more.
    let _again = NotesActor::open(cfg.clone(), Arc::clone(&store), clock)
        .unwrap_or_else(|e| panic!("reopen: {e}"));
    assert_eq!(ops_for(&store, &cfg.path).len(), 1);
}

#[test]
fn a_peers_ops_import_into_a_fresh_actor_and_land_on_disk() {
    let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    let (store_a, clock_a, cfg_a) = setup(dir.path(), 1);
    let mut a = NotesActor::open(cfg_a.clone(), Arc::clone(&store_a), clock_a)
        .unwrap_or_else(|e| panic!("{e}"));
    a.edit(
        "first\n",
        Principal::User {
            device: cfg_a.device,
        },
    )
    .unwrap_or_else(|e| panic!("{e}"));
    a.edit(
        "first\nsecond\n",
        Principal::User {
            device: cfg_a.device,
        },
    )
    .unwrap_or_else(|e| panic!("{e}"));
    let ops = ops_for(&store_a, &cfg_a.path);
    assert_eq!(ops.len(), 2);

    let (store_b, clock_b, cfg_b) = setup(dir.path(), 2);
    let mut b = NotesActor::open(cfg_b.clone(), Arc::clone(&store_b), clock_b)
        .unwrap_or_else(|e| panic!("{e}"));
    b.import_ops(ops).unwrap_or_else(|e| panic!("import: {e}"));
    assert_eq!(b.contents().0, b"first\nsecond\n");
    let on_disk = std::fs::read_to_string(&cfg_b.disk).unwrap_or_default();
    assert_eq!(on_disk, "first\nsecond\n");
    assert_eq!(
        ops_for(&store_b, &cfg_b.path).len(),
        2,
        "the batch is in b's log too"
    );
}

fn notes_op(n: u128, device: DeviceId, wall_ms: u64, edits: Vec<TextEdit>) -> Op {
    let path = FilePath::new("tasks/abc/notes.md").unwrap_or_else(|e| panic!("{e}"));
    Op {
        id: OpId::new(Ulid::from_u128(n)),
        hlc: Hlc {
            wall_ms,
            counter: 0,
            device,
        },
        principal: Principal::User { device },
        file: path.clone(),
        kind: OpKind::NotesEdit { file: path, edits },
    }
}

/// What a notes.md opened before v0.0.8 left behind (task notes-no-base): the base text was
/// adopted with no op, so the log's only op appends at char 5 of a text no op ever wrote.
fn legacy_log(store: &SharedStore, cfg: &NotesActorConfig) -> Op {
    let append = notes_op(
        900,
        cfg.device,
        500,
        vec![TextEdit::Insert {
            at: 5,
            text: "more\n".into(),
        }],
    );
    let bytes = b"base\nmore\n".to_vec();
    let projection = Projection {
        file: cfg.path.clone(),
        hash: hash_of(&bytes),
        bytes: bytes.clone(),
        written_at_ms: 500,
    };
    let mut guard = store.lock().unwrap_or_else(PoisonError::into_inner);
    guard
        .commit_change(std::slice::from_ref(&append), &projection, None)
        .unwrap_or_else(|e| panic!("{e}"));
    std::fs::write(&cfg.disk, &bytes).unwrap_or_else(|e| panic!("{e}"));
    append
}

#[test]
fn a_peers_op_that_does_not_fit_is_skipped_and_the_rest_of_the_batch_lands() {
    let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    let (store, clock, cfg) = setup(dir.path(), 2);
    let mut b =
        NotesActor::open(cfg.clone(), Arc::clone(&store), clock).unwrap_or_else(|e| panic!("{e}"));
    let peer = DeviceId::new(Ulid::from_u128(1));
    let past_the_end = notes_op(
        901,
        peer,
        500,
        vec![TextEdit::Insert {
            at: 8874,
            text: "x".into(),
        }],
    );
    let fits = notes_op(
        902,
        peer,
        600,
        vec![TextEdit::Insert {
            at: 0,
            text: "hi\n".into(),
        }],
    );
    b.import_ops(vec![past_the_end, fits])
        .unwrap_or_else(|e| panic!("one bad op must not refuse the batch: {e}"));
    assert_eq!(b.contents().0, b"hi\n");
    assert_eq!(
        ops_for(&store, &cfg.path).len(),
        2,
        "the skipped op stays in the log, so heads stay dense"
    );
}

#[test]
fn a_log_with_no_base_op_is_repaired_at_open_and_a_fresh_peer_gets_the_whole_file() {
    let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    let (store_a, clock_a, cfg_a) = setup(dir.path(), 1);
    legacy_log(&store_a, &cfg_a);
    let a = NotesActor::open(cfg_a.clone(), Arc::clone(&store_a), Arc::clone(&clock_a))
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(
        a.contents().0,
        b"base\nmore\n",
        "the repair leaves the text alone"
    );
    let ops = ops_for(&store_a, &cfg_a.path);
    assert_eq!(ops.len(), 2, "one repair op after the legacy append");
    assert!(matches!(ops[1].principal, Principal::External { .. }));
    let replayed = {
        let guard = store_a.lock().unwrap_or_else(PoisonError::into_inner);
        crate::notes_history::replay(&guard, &cfg_a.path, None).unwrap_or_else(|e| panic!("{e}"))
    };
    assert_eq!(
        replayed.text(),
        "base\nmore\n",
        "the log rebuilds the file now"
    );
    drop(a);
    let _again = NotesActor::open(cfg_a.clone(), Arc::clone(&store_a), clock_a)
        .unwrap_or_else(|e| panic!("reopen: {e}"));
    assert_eq!(
        ops_for(&store_a, &cfg_a.path).len(),
        2,
        "repaired once only"
    );

    let (store_b, clock_b, cfg_b) = setup(dir.path(), 2);
    let mut b = NotesActor::open(cfg_b.clone(), Arc::clone(&store_b), Arc::clone(&clock_b))
        .unwrap_or_else(|e| panic!("{e}"));
    b.import_ops(ops).unwrap_or_else(|e| panic!("import: {e}"));
    assert_eq!(b.contents().0, b"base\nmore\n");
    let on_disk = std::fs::read(&cfg_b.disk).unwrap_or_default();
    assert_eq!(on_disk, b"base\nmore\n");
    drop(b);
    let _b_again = NotesActor::open(cfg_b.clone(), Arc::clone(&store_b), clock_b)
        .unwrap_or_else(|e| panic!("reopen b: {e}"));
    assert_eq!(
        ops_for(&store_b, &cfg_b.path).len(),
        2,
        "the peer's log already rebuilds its text: no repair of its own"
    );
}
