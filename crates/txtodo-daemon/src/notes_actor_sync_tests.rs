//! `NotesActor` against its op log (task notes-sync, moved out of `notes_actor.rs` for its line
//! budget): the disk seed at open, and a peer's ops into a fresh actor.

use crate::actor::SharedStore;
use crate::clock::{Clock, FakeClock};
use crate::notes_actor::{NotesActor, NotesActorConfig};
use std::sync::{Arc, Mutex, PoisonError};
use txtodo_model::{DeviceId, FilePath, Op, Principal, Ulid};
use txtodo_store::{Seq, Store};

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
