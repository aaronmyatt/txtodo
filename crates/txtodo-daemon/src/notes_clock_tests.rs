//! `notes_clock.rs`: the notes actor's clock follows the stamps it holds.

use crate::clock::{Clock, FakeClock};
use crate::notes_actor::NotesActor;
use crate::notes_actor_sync_tests::{ops_for, setup};
use std::sync::Arc;
use txtodo_model::{Op, Principal};

/// Lab clock-skew 1072683562: b's clock is 2 minutes ahead (inside the skew bound). a takes b's
/// edit, then appends after it, before and after a restart: each append must sort after b's
/// edit, or the stamp-ordered rebuild (ADR 0034) slots it in front, where it does not fit.
#[test]
fn an_edit_after_a_peers_edit_from_a_clock_ahead_sorts_after_it() {
    let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    let (store_b, _, cfg_b) = setup(dir.path(), 2);
    let ahead: Arc<dyn Clock> = Arc::new(FakeClock::new(1_000 + 120_000));
    let mut b = NotesActor::open(cfg_b.clone(), Arc::clone(&store_b), ahead)
        .unwrap_or_else(|e| panic!("{e}"));
    let as_b = Principal::User {
        device: cfg_b.device,
    };
    b.edit("- from b\n", as_b).unwrap_or_else(|e| panic!("{e}"));

    let (store_a, clock_a, cfg_a) = setup(dir.path(), 1);
    let as_a = Principal::User {
        device: cfg_a.device,
    };
    let mut a = NotesActor::open(cfg_a.clone(), Arc::clone(&store_a), Arc::clone(&clock_a))
        .unwrap_or_else(|e| panic!("{e}"));
    a.import_ops(ops_for(&store_b, &cfg_b.path))
        .unwrap_or_else(|e| panic!("{e}"));
    a.edit("- from b\n- from a\n", as_a.clone())
        .unwrap_or_else(|e| panic!("{e}"));
    drop(a);
    let mut a = NotesActor::open(cfg_a.clone(), Arc::clone(&store_a), clock_a)
        .unwrap_or_else(|e| panic!("{e}"));
    a.edit("- from b\n- from a\n- again\n", as_a)
        .unwrap_or_else(|e| panic!("{e}"));

    let mut fresh = NotesActor::open(
        cfg_b.clone(),
        Arc::clone(&store_b),
        Arc::new(FakeClock::new(5)),
    )
    .unwrap_or_else(|e| panic!("{e}"));
    let theirs: Vec<Op> = ops_for(&store_a, &cfg_a.path)
        .into_iter()
        .filter(|op| op.hlc.device == cfg_a.device)
        .collect();
    fresh.import_ops(theirs).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(fresh.contents().0, b"- from b\n- from a\n- again\n");
    drop(fresh);
}
