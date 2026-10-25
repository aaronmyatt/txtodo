//! `Presence` (task `peer-prune`): a long gap between ticks moves `up_since`, a short one does not,
//! and a restart is judged against the saved tick time.

use txtodo_store::IdentityStore;

use super::{AWAY_GAP_MS, Presence};

/// The directory is returned too: dropping it would delete the database under the store.
fn store() -> (tempfile::TempDir, IdentityStore) {
    let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
    let store = IdentityStore::open(&dir.path().join("identity.db"))
        .unwrap_or_else(|e| panic!("store: {e}"));
    (dir, store)
}

#[test]
fn the_first_tick_of_a_fresh_device_counts_as_coming_back() {
    let (_dir, mut store) = store();
    let mut presence = Presence::load(&store);
    assert_eq!(presence.tick(&mut store, 5_000_000), 5_000_000);
}

#[test]
fn ticks_close_together_keep_up_since() {
    let (_dir, mut store) = store();
    let mut presence = Presence::load(&store);
    let start = 5_000_000;
    presence.tick(&mut store, start);
    let step = AWAY_GAP_MS / 2;
    assert_eq!(presence.tick(&mut store, start + step), start);
    assert_eq!(
        presence.tick(&mut store, start + 2 * step),
        start,
        "each gap is short"
    );
}

#[test]
fn a_gap_past_the_limit_moves_up_since_to_now() {
    let (_dir, mut store) = store();
    let mut presence = Presence::load(&store);
    presence.tick(&mut store, 5_000_000);
    let back = 5_000_000 + AWAY_GAP_MS + 1;
    assert_eq!(presence.tick(&mut store, back), back);
}

#[test]
fn a_restart_after_a_long_stop_counts_as_coming_back_and_a_quick_one_does_not() {
    let (_dir, mut store) = store();
    let mut presence = Presence::load(&store);
    let start = 5_000_000;
    presence.tick(&mut store, start);
    presence.tick(&mut store, start + 60_000);
    let mut quick = Presence::load(&store);
    assert_eq!(
        quick.tick(&mut store, start + 90_000),
        start,
        "back within the gap"
    );
    let mut slow = Presence::load(&store);
    let late = start + 60_000 + AWAY_GAP_MS + 1;
    assert_eq!(
        slow.tick(&mut store, late),
        late,
        "off for longer than the gap"
    );
}
