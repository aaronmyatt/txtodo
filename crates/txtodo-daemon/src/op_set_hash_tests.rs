//! `OpSetHash` (task sync-divergence-check): order-free, and each actor's running value always
//! equals a fresh fold over the store, after commits and across a reopen.

use crate::actor::{ActorConfig, FileActor, SharedStore};
use crate::clock::{Clock, FakeClock};
use crate::mutation::Mutation;
use crate::notes_actor::{NotesActor, NotesActorConfig};
use crate::op_set_hash::OpSetHash;
use crate::stats::Stats;
use std::path::Path;
use std::sync::{Arc, Mutex, PoisonError};
use txtodo_model::{DeviceId, FilePath, Hlc, IdentityMode, Op, OpId, OpKind, Principal, Ulid};
use txtodo_store::Store;

fn device() -> DeviceId {
    DeviceId::new(Ulid::from_u128(7))
}

fn file(name: &str) -> FilePath {
    FilePath::new(name).unwrap_or_else(|e| panic!("{e}"))
}

fn store(dir: &Path) -> SharedStore {
    Arc::new(Mutex::new(
        Store::open(&dir.join("oplog.db")).unwrap_or_else(|e| panic!("open store: {e}")),
    ))
}

fn clock() -> Arc<dyn Clock> {
    Arc::new(FakeClock::new(1_000))
}

fn stored(store: &SharedStore, name: &str) -> OpSetHash {
    let guard = store.lock().unwrap_or_else(PoisonError::into_inner);
    OpSetHash::of_file(&guard, &file(name)).unwrap_or_else(|e| panic!("{e}"))
}

fn op(n: u128, name: &str) -> Op {
    Op {
        id: OpId::new(Ulid::from_u128(n)),
        hlc: Hlc {
            wall_ms: 10,
            counter: u16::try_from(n).unwrap_or(0),
            device: device(),
        },
        principal: Principal::External { device: device() },
        file: file(name),
        kind: OpKind::BlankInsert { after: None },
    }
}

#[test]
fn arrival_order_does_not_change_the_hash() {
    let (a, b, c) = (op(1, "todo.txt"), op(2, "todo.txt"), op(3, "todo.txt"));
    let mut first = OpSetHash::default();
    first.add_ops(&[a.clone(), b.clone(), c.clone()]);
    let mut second = OpSetHash::default();
    second.add_ops(&[c.clone(), a.clone()]);
    second.add(b.id);
    assert_eq!(first, second);
    let mut fewer = OpSetHash::default();
    fewer.add_ops(&[a, c]);
    assert_ne!(first, fewer, "a missing op shows");
    assert_ne!(fewer, OpSetHash::default());

    // Two logs that got the same ops in different orders fold to the same hash.
    let (one, two) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let (s1, s2) = (store(one.path()), store(two.path()));
    let append = |s: &SharedStore, ops: &[Op]| {
        s.lock()
            .unwrap_or_else(PoisonError::into_inner)
            .append(ops)
            .unwrap_or_else(|e| panic!("{e}"));
    };
    append(
        &s1,
        &[op(1, "todo.txt"), op(2, "todo.txt"), op(9, "q4/todo.txt")],
    );
    append(&s2, &[op(2, "todo.txt")]);
    append(&s2, &[op(1, "todo.txt")]);
    assert_eq!(stored(&s1, "todo.txt"), stored(&s2, "todo.txt"));
    assert_ne!(stored(&s1, "q4/todo.txt"), stored(&s2, "q4/todo.txt"));
}

fn file_actor(dir: &Path, store: &SharedStore) -> FileActor {
    let cfg = ActorConfig {
        path: file("todo.txt"),
        disk: dir.join("todo.txt"),
        device: device(),
        stats: Arc::new(Stats::default()),
        identity_mode: IdentityMode::Sidecar,
        tree_dirty: Arc::new(crate::tree_dirty::TreeDirty::default()),
        layout: crate::layout_state::SharedLayout::default(),
    };
    FileActor::open(cfg, Arc::clone(store), clock()).unwrap_or_else(|e| panic!("open: {e}"))
}

#[test]
fn a_file_actor_keeps_its_op_set_in_step_with_the_log() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("todo.txt"), "buy milk\nwalk the dog\n").unwrap();
    let store = store(dir.path());
    let mut actor = file_actor(dir.path(), &store);
    let adopted = actor.op_set();
    assert_ne!(
        adopted,
        OpSetHash::default(),
        "adopting the file's lines made ops"
    );
    assert_eq!(adopted, stored(&store, "todo.txt"));

    let add = Mutation::Add {
        line: "call mum".into(),
    };
    let user = Principal::User { device: device() };
    actor
        .on_apply(vec![add], user, None)
        .unwrap_or_else(|e| panic!("apply: {e}"));
    let after = actor.op_set();
    assert_ne!(after, adopted);
    assert_eq!(after, stored(&store, "todo.txt"));

    drop(actor);
    assert_eq!(
        file_actor(dir.path(), &store).op_set(),
        after,
        "rebuilt at open"
    );
}

fn notes_actor(dir: &Path, store: &SharedStore) -> NotesActor {
    let cfg = NotesActorConfig {
        path: file("q4/abc/notes.md"),
        disk: dir.join("notes.md"),
        device: device(),
    };
    NotesActor::open(cfg, Arc::clone(store), clock()).unwrap_or_else(|e| panic!("open: {e}"))
}

#[test]
fn a_notes_actor_keeps_its_op_set_in_step_with_the_log() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("notes.md"), "first thoughts\n").unwrap();
    let store = store(dir.path());
    let mut actor = notes_actor(dir.path(), &store);
    let absorbed = actor.op_set();
    assert_ne!(
        absorbed,
        OpSetHash::default(),
        "the text on disk became one op"
    );
    assert_eq!(absorbed, stored(&store, "q4/abc/notes.md"));

    let user = Principal::User { device: device() };
    actor
        .edit("first thoughts\nsecond thoughts\n", user)
        .unwrap_or_else(|e| panic!("edit: {e}"));
    let after = actor.op_set();
    assert_ne!(after, absorbed);
    assert_eq!(after, stored(&store, "q4/abc/notes.md"));

    drop(actor);
    assert_eq!(
        notes_actor(dir.path(), &store).op_set(),
        after,
        "rebuilt at open"
    );
}
