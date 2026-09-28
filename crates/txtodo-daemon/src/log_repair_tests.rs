//! `FileActor::repair_log` (task todo-log-repair): a log that does not rebuild its file gets
//! repair ops at open, once, without touching the file, and a fresh peer that imports the log
//! lands on the file. Both shapes seen on this repo's real backlog on 2026-09-28: bytes adopted
//! with no op behind them, and a delete recorded twice (one blank line too many on replay).

use crate::actor::{ActorConfig, FileActor, SharedStore, hash_of};
use crate::clock::{Clock, FakeClock};
use crate::stats::Stats;
use std::path::Path;
use std::sync::{Arc, Mutex, PoisonError};
use txtodo_model::{
    DeviceId, Field, FieldValue, FilePath, Hlc, IdentityMode, Op, OpId, OpKind, Principal, TaskId,
    Ulid, set_field,
};
use txtodo_store::{Projection, Seq, Store};

fn device(n: u128) -> DeviceId {
    DeviceId::new(Ulid::from_u128(n))
}

fn path() -> FilePath {
    FilePath::new("todo.txt").unwrap_or_else(|e| panic!("{e}"))
}

fn task(n: u128) -> TaskId {
    TaskId::new(Ulid::from_u128(n))
}

fn store(dir: &Path) -> SharedStore {
    Arc::new(Mutex::new(
        Store::open(&dir.join("oplog.db")).unwrap_or_else(|e| panic!("open store: {e}")),
    ))
}

fn open(dir: &Path, store: &SharedStore, me: DeviceId) -> FileActor {
    let clock: Arc<dyn Clock> = Arc::new(FakeClock::new(9_000));
    let cfg = ActorConfig {
        path: path(),
        disk: dir.join("todo.txt"),
        device: me,
        stats: Arc::new(Stats::default()),
        identity_mode: IdentityMode::Tagged,
        tree_dirty: Arc::new(crate::tree_dirty::TreeDirty::default()),
        layout: crate::layout_state::SharedLayout::default(),
    };
    FileActor::open(cfg, Arc::clone(store), clock).unwrap_or_else(|e| panic!("open actor: {e}"))
}

fn op(n: u128, kind: OpKind) -> Op {
    Op {
        id: OpId::new(Ulid::from_u128(1_000 + n)),
        hlc: Hlc {
            wall_ms: 5_000 + n as u64,
            counter: 0,
            device: device(1),
        },
        principal: Principal::External { device: device(1) },
        file: path(),
        kind,
    }
}

fn insert(n: u128, t: TaskId, after: Option<TaskId>, text: &str) -> Op {
    op(
        n,
        OpKind::Insert {
            task: t,
            after,
            line: format!("{text} id:{t}"),
        },
    )
}

fn deleted(n: u128, t: TaskId) -> Op {
    let kind =
        set_field(t, Field::Deleted, FieldValue::Bool(true)).unwrap_or_else(|e| panic!("{e}"));
    op(n, kind)
}

/// A log and a file that disagree, as an older build left them (file adopted, ops short of it).
fn seed(dir: &Path, store: &SharedStore, ops: &[Op], file: &str) {
    let bytes = file.as_bytes().to_vec();
    let projection = Projection {
        file: path(),
        hash: hash_of(&bytes),
        bytes: bytes.clone(),
        written_at_ms: 5_000,
    };
    store
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .commit_change(ops, &projection, None)
        .unwrap_or_else(|e| panic!("{e}"));
    std::fs::write(dir.join("todo.txt"), &bytes).unwrap_or_else(|e| panic!("{e}"));
}

fn ops_of(store: &SharedStore) -> Vec<Op> {
    let guard = store.lock().unwrap_or_else(PoisonError::into_inner);
    guard
        .for_file(&path(), Seq(0))
        .unwrap_or_else(|e| panic!("{e}"))
        .into_iter()
        .map(|s| s.op)
        .collect()
}

/// Repaired once at open, file and history untouched, and a fresh peer lands on the file.
fn assert_repaired_and_a_peer_converges(ops: &[Op], file: &str) {
    let a_dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    let a_store = store(a_dir.path());
    seed(a_dir.path(), &a_store, ops, file);
    let a = open(a_dir.path(), &a_store, device(1));
    assert_eq!(
        a.projection,
        file.as_bytes(),
        "the repair leaves the text alone"
    );
    let on_disk = std::fs::read(a_dir.path().join("todo.txt")).unwrap_or_default();
    assert_eq!(on_disk, file.as_bytes(), "and the file");
    let logged = ops_of(&a_store);
    assert!(logged.len() > ops.len(), "repair ops were committed");
    let history = {
        let guard = a_store.lock().unwrap_or_else(PoisonError::into_inner);
        crate::history::replay(&guard, &path(), None, IdentityMode::Tagged)
            .unwrap_or_else(|e| panic!("history replay: {e}"))
    };
    assert_eq!(
        history.to_bytes(),
        file.as_bytes(),
        "history starts from the file"
    );
    drop(a);
    let _again = open(a_dir.path(), &a_store, device(1));
    assert_eq!(ops_of(&a_store).len(), logged.len(), "repaired once only");

    let b_dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    let b_store = store(b_dir.path());
    let mut b = open(b_dir.path(), &b_store, device(2));
    b.on_sync_ops(logged)
        .unwrap_or_else(|e| panic!("import: {e}"));
    assert_eq!(
        String::from_utf8_lossy(&b.projection),
        file,
        "a fresh peer rebuilds the whole file"
    );
}

#[test]
fn bytes_adopted_with_no_op_behind_them_are_repaired() {
    let (t1, t2) = (task(1), task(2));
    // The log only knows `thing`; the file has it done with its creation date kept, plus a
    // line no op ever inserted.
    let ops = [insert(1, t1, None, "2026-09-25 thing +m11")];
    let file = format!("x 2026-09-25 2026-09-25 thing +m11 id:{t1}\nother id:{t2}\n");
    assert_repaired_and_a_peer_converges(&ops, &file);
}

#[test]
fn a_delete_recorded_twice_is_repaired() {
    let (t1, t2) = (task(1), task(2));
    // One delete, logged by two daemons: each copy leaves a blank line on replay.
    let ops = [
        insert(1, t1, None, "stale copy"),
        insert(2, t2, Some(t1), "kept"),
        deleted(3, t1),
        op(4, OpKind::BlankInsert { after: None }),
        deleted(5, t1),
        op(6, OpKind::BlankInsert { after: None }),
    ];
    let file = format!("\nkept id:{t2}\n");
    assert_repaired_and_a_peer_converges(&ops, &file);
}

#[test]
fn a_log_that_rebuilds_its_file_gets_no_repair() {
    let t1 = task(1);
    let ops = [insert(1, t1, None, "fine")];
    let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    let s = store(dir.path());
    seed(dir.path(), &s, &ops, &format!("fine id:{t1}\n"));
    let _a = open(dir.path(), &s, device(1));
    assert_eq!(ops_of(&s).len(), 1, "nothing to repair");
}
