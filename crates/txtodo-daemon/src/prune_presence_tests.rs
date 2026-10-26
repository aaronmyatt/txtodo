//! `Presence` (task `peer-prune`): a long gap between ticks moves `up_since`, a short one does not,
//! and a restart is judged against the saved tick time.

use txtodo_store::IdentityStore;

use super::{AWAY_GAP_MS, Presence, away_gap_ms};
use crate::prune_stale::{DAY_MS, PruneConfig};

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
    assert_eq!(presence.tick(&mut store, 5_000_000, AWAY_GAP_MS), 5_000_000);
}

#[test]
fn ticks_close_together_keep_up_since() {
    let (_dir, mut store) = store();
    let mut presence = Presence::load(&store);
    let start = 5_000_000;
    presence.tick(&mut store, start, AWAY_GAP_MS);
    let step = AWAY_GAP_MS / 2;
    assert_eq!(presence.tick(&mut store, start + step, AWAY_GAP_MS), start);
    assert_eq!(
        presence.tick(&mut store, start + 2 * step, AWAY_GAP_MS),
        start,
        "each gap is short"
    );
}

#[test]
fn a_gap_past_the_limit_moves_up_since_to_now() {
    let (_dir, mut store) = store();
    let mut presence = Presence::load(&store);
    presence.tick(&mut store, 5_000_000, AWAY_GAP_MS);
    let back = 5_000_000 + AWAY_GAP_MS + 1;
    assert_eq!(presence.tick(&mut store, back, AWAY_GAP_MS), back);
}

#[test]
fn a_restart_after_a_long_stop_counts_as_coming_back_and_a_quick_one_does_not() {
    let (_dir, mut store) = store();
    let mut presence = Presence::load(&store);
    let start = 5_000_000;
    presence.tick(&mut store, start, AWAY_GAP_MS);
    presence.tick(&mut store, start + 60_000, AWAY_GAP_MS);
    let mut quick = Presence::load(&store);
    assert_eq!(
        quick.tick(&mut store, start + 90_000, AWAY_GAP_MS),
        start,
        "back within the gap"
    );
    let mut slow = Presence::load(&store);
    let late = start + 60_000 + AWAY_GAP_MS + 1;
    assert_eq!(
        slow.tick(&mut store, late, AWAY_GAP_MS),
        late,
        "off for longer than the gap"
    );
}

#[test]
fn the_away_gap_is_ten_minutes_unless_a_short_threshold_halves_below_it() {
    assert_eq!(away_gap_ms(&PruneConfig::default()), AWAY_GAP_MS);
    let short = PruneConfig {
        parked_after_ms: DAY_MS,
        quiet_after_ms: 20_000,
    };
    assert_eq!(away_gap_ms(&short), 10_000);
    let one_off = PruneConfig {
        parked_after_ms: 0,
        quiet_after_ms: 20_000,
    };
    assert_eq!(away_gap_ms(&one_off), 10_000, "an off rule does not count");
    let both_off = PruneConfig {
        parked_after_ms: 0,
        quiet_after_ms: 0,
    };
    assert_eq!(away_gap_ms(&both_off), AWAY_GAP_MS);
}
