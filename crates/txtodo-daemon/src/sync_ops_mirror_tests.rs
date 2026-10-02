//! `FileActor::on_sync_ops` and the Loro mirror: what a synced batch feeds it, and which heals
//! count as errors (the lab's tripwire). Split from `sync_ops_tests.rs` for the file budget.

use crate::clock::FakeClock;
use crate::sync_ops_tests::{insert, op_from, open, store};
use std::sync::Arc;
use txtodo_model::{TaskId, Ulid};

/// Lab chaos seed 202 (report 20261002-184539-chaos): b1 logged `mirror_refused_converging` for
/// its own add after a line another device had not delivered yet, arriving through a Remote mirror
/// of the shared list. The document parked the add, but the commit still fed it to the mirror,
/// which lacks that line too. Now the mirror gets only what applied, and heals once, when it lands.
#[test]
fn the_mirror_is_not_fed_an_op_that_waits() {
    let dir = tempfile::tempdir().unwrap();
    let store = store(dir.path());
    let clock = Arc::new(FakeClock::new(1_000));
    let mut actor = open(dir.path(), &store, &clock);
    let (a, b, c) = (
        TaskId::new(Ulid::from_u128(95)),
        TaskId::new(Ulid::from_u128(96)),
        TaskId::new(Ulid::from_u128(97)),
    );
    let heals_at_open = actor.mirror_heals;
    // Own-device ops (device 1), as a Remote mirror relays them: b fits, c needs a.
    actor
        .on_sync_ops(vec![
            op_from(11, 1, 3_000, insert(b, None, "b")),
            op_from(12, 1, 3_001, insert(c, Some(a), "c")),
        ])
        .unwrap();
    assert_eq!(actor.parked.len(), 1);
    assert_eq!(actor.mirror_heals, heals_at_open, "no refusal, no heal");
    assert!(actor.mirror.agrees_with(&actor.state));
    actor
        .on_sync_ops(vec![op_from(10, 7, 2_000, insert(a, None, "a"))])
        .unwrap();
    assert_eq!(actor.parked.len(), 0);
    assert_eq!(
        actor.mirror_heals,
        heals_at_open + 1,
        "one heal once c lands"
    );
    assert!(actor.mirror.agrees_with(&actor.state));
}

/// Lab chaos: b1's own ops, relayed back in its Remote mirror of a list it shares, logged
/// `mirror_flush_disagreed_converging` in every run. A synced batch that re-homes a line leaves
/// the mirror apart from the state as expected (ADR 0033), whichever device made its ops: the
/// heal still runs, but not as an error.
#[test]
fn our_own_ops_back_through_sync_heal_quietly() {
    let dir = tempfile::tempdir().unwrap();
    let store = store(dir.path());
    let clock = Arc::new(FakeClock::new(1_000));
    let mut actor = open(dir.path(), &store, &clock);
    let (a, b, c) = (
        TaskId::new(Ulid::from_u128(81)),
        TaskId::new(Ulid::from_u128(82)),
        TaskId::new(Ulid::from_u128(83)),
    );
    // All own-device ops (device 1); b arrives after c but is stamped before it.
    actor
        .on_sync_ops(vec![
            op_from(21, 1, 2_000, insert(a, None, "a")),
            op_from(23, 1, 3_000, insert(c, Some(a), "c")),
        ])
        .unwrap();
    let heals = actor.mirror_heals;
    actor
        .on_sync_ops(vec![op_from(22, 1, 2_500, insert(b, Some(a), "b"))])
        .unwrap();
    assert_eq!(actor.mirror_heals, heals + 1, "the mirror needed a heal");
    assert_eq!(actor.mirror_errors, 0, "an expected gap, not an error");
    assert!(actor.mirror.agrees_with(&actor.state));
}
