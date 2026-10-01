//! Task notes-watch: a notes.md write that would land over an editor's unmerged save is held,
//! and the watcher's event (`absorb_disk`) merges that save three-way with what the actor
//! committed meanwhile (`notes_held.rs`).

use crate::clock::{Clock, FakeClock};
use crate::notes_actor::{NotesActor, NotesActorConfig};
use std::path::Path;
use std::sync::{Arc, Mutex};
use txtodo_model::{DeviceId, FilePath, Principal, Ulid};
use txtodo_store::Store;

const ME: u128 = 1;

/// One actor on `dir/notes.md` holding `text`, written to disk.
fn actor_with(dir: &Path, text: &str) -> (NotesActor, std::path::PathBuf) {
    let store = Arc::new(Mutex::new(
        Store::open(&dir.join("oplog.db")).unwrap_or_else(|e| panic!("open store: {e}")),
    ));
    let clock: Arc<dyn Clock> = Arc::new(FakeClock::new(1_000));
    let disk = dir.join("notes.md");
    let cfg = NotesActorConfig {
        path: FilePath::new("q4/abc/notes.md").unwrap_or_else(|e| panic!("{e}")),
        disk: disk.clone(),
        device: DeviceId::new(Ulid::from_u128(ME)),
    };
    let mut actor = NotesActor::open(cfg, store, clock).unwrap_or_else(|e| panic!("open: {e}"));
    actor
        .edit(text, me())
        .unwrap_or_else(|e| panic!("seed: {e}"));
    (actor, disk)
}

fn me() -> Principal {
    Principal::User {
        device: DeviceId::new(Ulid::from_u128(ME)),
    }
}

fn text(actor: &NotesActor) -> String {
    String::from_utf8(actor.contents().0).unwrap_or_default()
}

fn on_disk(disk: &Path) -> String {
    std::fs::read_to_string(disk).unwrap_or_default()
}

#[test]
fn an_edit_over_an_unmerged_save_is_held_then_merged_with_it() {
    let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    let (mut actor, disk) = actor_with(dir.path(), "one\ntwo\n");
    std::fs::write(&disk, "one\ntwo\nthree\n").unwrap_or_else(|e| panic!("{e}"));

    actor
        .edit("zero\none\ntwo\n", me())
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(
        on_disk(&disk),
        "one\ntwo\nthree\n",
        "the save is not written over"
    );

    actor.absorb_disk().unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(text(&actor), "zero\none\ntwo\nthree\n");
    assert_eq!(on_disk(&disk), text(&actor));
}

#[test]
fn every_edit_while_held_waits_for_the_one_merge() {
    let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    let (mut actor, disk) = actor_with(dir.path(), "b\n");
    std::fs::write(&disk, "b\nsaved\n").unwrap_or_else(|e| panic!("{e}"));
    actor.edit("a\nb\n", me()).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(on_disk(&disk), "b\nsaved\n");
    // The next write merges the held save first, then lands on the merged text.
    actor
        .edit("a\nb\nsaved\nc\n", me())
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(text(&actor), "a\nb\nsaved\nc\n");
    assert_eq!(on_disk(&disk), text(&actor));
}

#[test]
fn a_save_put_back_to_what_we_wrote_releases_the_held_write() {
    let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    let (mut actor, disk) = actor_with(dir.path(), "kept\n");
    std::fs::write(&disk, "kept\nundone soon\n").unwrap_or_else(|e| panic!("{e}"));
    actor
        .edit("kept\nmine\n", me())
        .unwrap_or_else(|e| panic!("{e}"));
    std::fs::write(&disk, "kept\n").unwrap_or_else(|e| panic!("{e}"));

    actor.absorb_disk().unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(text(&actor), "kept\nmine\n");
    assert_eq!(on_disk(&disk), "kept\nmine\n");
}

#[test]
fn a_held_base_survives_a_restart_and_the_merge_resumes() {
    let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    let store = Arc::new(Mutex::new(
        Store::open(&dir.path().join("oplog.db")).unwrap_or_else(|e| panic!("open store: {e}")),
    ));
    let disk = dir.path().join("notes.md");
    let cfg = NotesActorConfig {
        path: FilePath::new("q4/abc/notes.md").unwrap_or_else(|e| panic!("{e}")),
        disk: disk.clone(),
        device: DeviceId::new(Ulid::from_u128(ME)),
    };
    let open = |clock_ms: u64| {
        let clock: Arc<dyn Clock> = Arc::new(FakeClock::new(clock_ms));
        NotesActor::open(cfg.clone(), Arc::clone(&store), clock)
            .unwrap_or_else(|e| panic!("open: {e}"))
    };
    let mut actor = open(1_000);
    actor
        .edit("one\ntwo\n", me())
        .unwrap_or_else(|e| panic!("{e}"));
    std::fs::write(&disk, "one\ntwo\nthree\n").unwrap_or_else(|e| panic!("{e}"));
    actor
        .edit("zero\none\ntwo\n", me())
        .unwrap_or_else(|e| panic!("{e}"));
    drop(actor); // stopped while the write is held: the disk never got "zero"

    let reopened = open(5_000);
    assert_eq!(
        text(&reopened),
        "zero\none\ntwo\nthree\n",
        "neither side lost"
    );
    assert_eq!(on_disk(&disk), text(&reopened));
}
