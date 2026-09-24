//! `reconcile_replay.rs` (task `sync-poison-op`): synthesized ops reproduce the target's lines and
//! replay on a copy of the current state; an external edit the reconciler cannot express exactly
//! still commits ops a peer can replay from scratch.

use std::sync::{Arc, Mutex};

use crate::actor::{ActorConfig, FileActor, SharedStore};
use crate::clock::FakeClock;
use crate::reconcile_replay::replayable_ops;
use crate::state::{DocState, id_of};
use txtodo_core::parse_file;
use txtodo_model::{DeviceId, FilePath, IdentityMode, TaskId, Ulid};
use txtodo_store::{Seq, Store};

fn path() -> FilePath {
    FilePath::new("todo.txt").unwrap_or_else(|e| panic!("{e}"))
}

fn tid(n: u128) -> TaskId {
    TaskId::new(Ulid::from_u128(1 << 80 | n))
}

/// A tagged line: `name id:<ulid n>`.
fn t(n: u128, name: &str) -> String {
    format!("{name} id:{}", tid(n))
}

fn doc(lines: &[String]) -> DocState {
    let text: String = lines.iter().map(|l| format!("{l}\n")).collect();
    let file = parse_file(text.as_bytes());
    let ids: Vec<Option<TaskId>> = file.lines.iter().map(id_of).collect();
    DocState::from_file(path(), &file, &ids, IdentityMode::Tagged)
        .unwrap_or_else(|e| panic!("doc: {e}"))
}

/// Replays `ops` on `current` the way a peer would and checks it lands on `target`'s bytes.
fn assert_replays(current: &DocState, target: &DocState) {
    let (ops, _) = replayable_ops(current, target).unwrap_or_else(|| panic!("no ops"));
    let mut peer = current.clone();
    for kind in &ops {
        peer.apply(&crate::fastid::hydration_op(&path(), kind.clone()))
            .unwrap_or_else(|e| panic!("{kind:?} does not replay: {e}"));
    }
    assert_eq!(
        String::from_utf8_lossy(&peer.to_bytes()),
        String::from_utf8_lossy(&target.to_bytes())
    );
}

#[test]
fn reorder_insert_delete_and_blanks_replay_to_the_target() {
    let current = doc(&[
        t(1, "a"),
        String::new(),
        t(2, "b"),
        t(3, "c"),
        String::new(),
    ]);
    let target = doc(&[
        String::new(),
        t(3, "c"),
        t(4, "new"),
        String::new(),
        String::new(),
        t(1, "a"),
    ]);
    assert_replays(&current, &target);
}

#[test]
fn a_changed_line_replays_through_field_ops_or_a_reinsert() {
    let current = doc(&[t(1, "(A) walk the dog"), t(2, "feed the cat")]);
    let target = doc(&[
        t(2, "x 2026-09-25 feed the cat"),
        t(1, "(B) walk the dog twice"),
    ]);
    assert_replays(&current, &target);
}

#[test]
fn a_move_after_a_task_the_same_edit_inserts_replays() {
    // The seq-23778 shape: the reconciler moved task 1 after task 9 before inserting task 9.
    let current = doc(&[t(1, "one"), t(2, "two")]);
    let target = doc(&[t(2, "two"), t(9, "nine"), t(1, "one")]);
    assert_replays(&current, &target);
}

#[test]
fn a_task_id_twice_in_the_target_gives_no_ops_rather_than_wrong_ones() {
    let current = doc(&[t(1, "one")]);
    let target = doc(&[t(1, "one"), t(2, "two"), t(1, "one")]);
    assert!(replayable_ops(&current, &target).is_none());
}

fn actor(dir: &std::path::Path) -> (FileActor, SharedStore) {
    let store: SharedStore = Arc::new(Mutex::new(
        Store::open(&dir.join("oplog.db")).unwrap_or_else(|e| panic!("store: {e}")),
    ));
    let cfg = ActorConfig {
        path: path(),
        disk: dir.join("todo.txt"),
        device: DeviceId::new(Ulid::from_u128(7)),
        stats: Arc::new(crate::stats::Stats::default()),
        identity_mode: IdentityMode::Tagged,
        tree_dirty: Arc::new(crate::tree_dirty::TreeDirty::default()),
        layout: crate::layout_state::SharedLayout::default(),
    };
    let clock: Arc<dyn crate::clock::Clock> = Arc::new(FakeClock::new(1_000));
    let actor = FileActor::open(cfg, Arc::clone(&store), clock)
        .unwrap_or_else(|e| panic!("open actor: {e}"));
    (actor, store)
}

#[test]
fn an_edit_the_reconciler_cannot_express_still_commits_ops_a_peer_replays() {
    let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    let before = format!("{}\n\n{}\n", t(1, "a"), t(2, "b"));
    std::fs::write(dir.path().join("todo.txt"), &before).unwrap_or_else(|e| panic!("{e}"));
    let (mut actor, store) = actor(dir.path());
    // A line after the blank: the reconciler anchors it after `a`, above the blank.
    let after = format!("{}\n\n{}\n{}\n", t(1, "a"), t(3, "c"), t(2, "b"));
    std::fs::write(dir.path().join("todo.txt"), &after).unwrap_or_else(|e| panic!("{e}"));
    actor
        .on_external_change()
        .unwrap_or_else(|e| panic!("reconcile: {e}"));

    let ops = store
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .for_file(&path(), Seq(0))
        .unwrap_or_else(|e| panic!("read ops: {e}"));
    let mut peer = doc(&[]);
    for stored in &ops {
        peer.apply(&stored.op)
            .unwrap_or_else(|e| panic!("{:?} does not replay: {e}", stored.op.kind));
    }
    let disk = std::fs::read(dir.path().join("todo.txt")).unwrap_or_default();
    assert_eq!(
        String::from_utf8_lossy(&peer.to_bytes()),
        String::from_utf8_lossy(&disk)
    );
}
