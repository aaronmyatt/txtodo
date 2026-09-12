//! Two `NotesActor`s that share a Loro lineage (B seeded from A's mirror snapshot, as pairing
//! will do for notes too) edit different parts of the text offline; B imports A's updates and both
//! sides end up with the merged text — same shape as `import_tests.rs`'s paired-actors harness.

use crate::clock::{Clock, FakeClock};
use crate::notes_actor::{NotesActor, NotesActorConfig};
use std::path::Path;
use std::sync::{Arc, Mutex};
use txtodo_model::{DeviceId, FilePath, Principal, Ulid};
use txtodo_store::Store;

fn dev(n: u128) -> DeviceId {
    DeviceId::new(Ulid::from_u128(n))
}

fn path() -> FilePath {
    FilePath::new("q4/abc/notes.md").unwrap_or_else(|e| panic!("{e}"))
}

fn store(dir: &Path) -> Arc<Mutex<Store>> {
    Arc::new(Mutex::new(
        Store::open(&dir.join("oplog.db")).unwrap_or_else(|e| panic!("open store: {e}")),
    ))
}

fn open(
    dir: &Path,
    device: DeviceId,
    store: &Arc<Mutex<Store>>,
    clock: &Arc<FakeClock>,
) -> NotesActor {
    let clock: Arc<dyn Clock> = clock.clone();
    let cfg = NotesActorConfig {
        path: path(),
        disk: dir.join("notes.md"),
        device,
    };
    NotesActor::open(cfg, Arc::clone(store), clock).unwrap_or_else(|e| panic!("open actor: {e}"))
}

/// A with seed text on disk, and B seeded from A's mirror snapshot at seq 0 — what pairing ships
/// as "snapshot plus ops" (same shape as `import_tests.rs`'s `paired()`).
fn paired() -> (tempfile::TempDir, NotesActor, tempfile::TempDir, NotesActor) {
    let seed = "one two three four five\n";
    let a_dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    std::fs::write(a_dir.path().join("notes.md"), seed).unwrap_or_else(|e| panic!("{e}"));
    let clock = Arc::new(FakeClock::new(1_000));
    let a_store = store(a_dir.path());
    let a = open(a_dir.path(), dev(1), &a_store, &clock);
    let snapshot = a.mirror_snapshot();

    let b_dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    std::fs::write(b_dir.path().join("notes.md"), seed).unwrap_or_else(|e| panic!("{e}"));
    let b_store = store(b_dir.path());
    {
        let mut s = b_store.lock().unwrap_or_else(|e| panic!("{e}"));
        s.put_mirror(&path(), &snapshot, txtodo_store::Seq(0))
            .unwrap_or_else(|e| panic!("{e}"));
    }
    let b = open(b_dir.path(), dev(2), &b_store, &clock);
    (a_dir, a, b_dir, b)
}

#[test]
fn concurrent_edits_to_different_parts_of_the_text_merge_without_data_loss() {
    let (_a_dir, mut a, _b_dir, mut b) = paired();
    let principal_a = Principal::User { device: dev(1) };
    let principal_b = Principal::User { device: dev(2) };

    // A prepends a word; B appends a word — different offsets, offline from each other.
    a.edit("zero one two three four five\n", principal_a)
        .unwrap_or_else(|e| panic!("{e}"));
    b.edit("one two three four five six\n", principal_b)
        .unwrap_or_else(|e| panic!("{e}"));

    // B pulls A's updates and merges them into its own already-edited text.
    let since = b.version();
    let updates = a.export_since(&since).unwrap_or_else(|e| panic!("{e}"));
    let applied = b
        .import_updates(&updates, dev(1))
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(applied.applied, 1, "one derived log entry for the merge");

    let (bytes, _hash) = b.contents();
    let merged = String::from_utf8(bytes).unwrap_or_else(|e| panic!("{e}"));
    assert!(merged.contains("zero"), "{merged}");
    assert!(merged.contains("six"), "{merged}");
    assert_eq!(merged, "zero one two three four five six\n");
}

#[test]
fn a_no_op_edit_applies_nothing() {
    let (_a_dir, mut a, _b_dir, _b) = paired();
    let principal = Principal::User { device: dev(1) };
    let applied = a
        .edit("one two three four five\n", principal)
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(applied.applied, 0, "identical text is not an edit");
}
