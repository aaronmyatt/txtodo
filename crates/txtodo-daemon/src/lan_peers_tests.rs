//! `lan_peers.rs`'s dial bookkeeping, driven the way `relay_autodial::spawn_resync_dial` and
//! `lan.rs::dial_and_spawn` drive it: a fake clock value in, `due`/`backoff` decisions out.
//! Ref: <https://doc.rust-lang.org/std/sync/struct.Mutex.html>

use std::sync::{Arc, Mutex};

use txtodo_model::{DeviceId, Ulid};
use txtodo_sync::backoff_ms;

use crate::lan_peers::{DialState, SharedDialState, record_dial_outcome, try_begin_dial};

fn peer() -> DeviceId {
    DeviceId::new(Ulid::from_u128(7))
}

fn fresh() -> SharedDialState {
    Arc::new(Mutex::new(DialState::default()))
}

#[test]
fn a_fresh_peer_is_dialed_at_once_and_not_again_within_its_backoff() {
    let state = fresh();
    assert!(try_begin_dial(&state, peer(), 1_000), "first tick dials");
    assert!(
        !try_begin_dial(&state, peer(), 1_000 + backoff_ms(0) - 1),
        "a second tick inside the base backoff is skipped"
    );
    assert!(
        try_begin_dial(&state, peer(), 1_000 + backoff_ms(0)),
        "due again once the base backoff has elapsed"
    );
}

#[test]
fn a_failed_dial_pushes_the_next_one_out_exponentially() {
    let state = fresh();
    assert!(try_begin_dial(&state, peer(), 0));
    record_dial_outcome(&state, peer(), false);
    assert!(
        try_begin_dial(&state, peer(), backoff_ms(1)),
        "one failure: 2x base"
    );
    record_dial_outcome(&state, peer(), false);
    let again = backoff_ms(1) + backoff_ms(2);
    assert!(
        !try_begin_dial(&state, peer(), again - 1),
        "two failures: 4x base, not yet"
    );
    assert!(try_begin_dial(&state, peer(), again));
}

#[test]
fn a_greeted_session_resets_the_backoff() {
    let state = fresh();
    assert!(try_begin_dial(&state, peer(), 0));
    record_dial_outcome(&state, peer(), false);
    record_dial_outcome(&state, peer(), false);
    assert!(try_begin_dial(&state, peer(), backoff_ms(2)));
    record_dial_outcome(&state, peer(), true);
    assert!(
        try_begin_dial(&state, peer(), backoff_ms(2) + backoff_ms(0)),
        "after a success only the base backoff applies again"
    );
}

#[test]
fn peers_back_off_independently() {
    let state = fresh();
    let other = DeviceId::new(Ulid::from_u128(8));
    assert!(try_begin_dial(&state, peer(), 0));
    record_dial_outcome(&state, peer(), false);
    assert!(
        try_begin_dial(&state, other, 0),
        "the other peer is untouched"
    );
    assert!(!try_begin_dial(&state, peer(), backoff_ms(0)));
}
