//! One prune round (task `peer-prune`, ADR 0044): a stale peer is removed through `remove_device`
//! and the group key rotates, a fresh one stays, the last peer may go, and a removal that fails
//! leaves the peer in place and is warned about once.

use std::collections::BTreeSet;

use txtodo_model::{DeviceId, Ulid};
use txtodo_store::NewDevice;

use super::prune_once;
use crate::device_identity::DeviceIdentity;
use crate::group_rotation::is_removed;
use crate::live_peers::Carrier;
use crate::peer_keys_dial_tests::identity_in;
use crate::prune_stale::{DAY_MS, PruneConfig};
use crate::removal_why::{self, Removal, Why};

const NOW: u64 = 30 * DAY_MS;

fn device(n: u128) -> DeviceId {
    DeviceId::new(Ulid::from_u128(n))
}

fn register(identity: &DeviceIdentity, n: u128, seen_ms: u64) {
    let new = NewDevice {
        device: device(n),
        name: String::new(),
        static_public: [n as u8; txtodo_store::DEVICE_STATIC_KEY_BYTES],
        paired_at_ms: seen_ms,
        last_known_wall_ms: None,
        key_epoch: 0,
    };
    let mut store = identity.store().lock().unwrap_or_else(|e| e.into_inner());
    store
        .register_device_as(&new, true)
        .unwrap_or_else(|e| panic!("register: {e}"));
}

fn round(identity: &DeviceIdentity, failed: &mut BTreeSet<DeviceId>) {
    prune_once(identity, &PruneConfig::default(), (NOW, 0), failed);
}

#[test]
fn a_stale_peer_is_removed_and_the_key_rotates_for_the_one_that_stays() {
    let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
    let identity = identity_in(dir.path(), [7; 32]);
    register(&identity, 1, NOW - 8 * DAY_MS);
    register(&identity, 2, NOW - 1_000);
    round(&identity, &mut BTreeSet::new());
    assert!(is_removed(&identity, device(1)), "silent for 8 days");
    let why = removal_why::read(&identity.store().lock().unwrap(), device(1));
    assert_eq!(
        why,
        Some(Removal {
            why: Why::PrunedQuiet,
            by: identity.device()
        })
    );
    assert!(!is_removed(&identity, device(2)), "heard a second ago");
    assert_eq!(identity.group_epoch(), 1, "one rotation, for the kept peer");
}

#[test]
fn nobody_is_pruned_while_every_peer_is_fresh() {
    let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
    let identity = identity_in(dir.path(), [7; 32]);
    register(&identity, 1, NOW - DAY_MS);
    register(&identity, 2, NOW - 2 * DAY_MS);
    round(&identity, &mut BTreeSet::new());
    assert_eq!(identity.group_epoch(), 0);
    assert!(!is_removed(&identity, device(1)) && !is_removed(&identity, device(2)));
}

#[test]
fn the_last_peer_may_go_and_the_key_moves_on_without_it() {
    let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
    let identity = identity_in(dir.path(), [7; 32]);
    register(&identity, 1, NOW - 9 * DAY_MS);
    round(&identity, &mut BTreeSet::new());
    assert!(is_removed(&identity, device(1)));
    assert_eq!(identity.group_epoch(), 1, "a key only this device holds");
}

#[test]
fn a_removal_that_fails_leaves_the_peer_and_is_booked_once() {
    let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
    let identity = identity_in(dir.path(), [7; 32]);
    register(&identity, 1, NOW - 8 * DAY_MS);
    register(&identity, 2, NOW - 1_000);
    // There is no epoch after the last one, so every rotation fails.
    identity.set_group_epoch(u32::MAX);
    let mut failed = BTreeSet::new();
    round(&identity, &mut failed);
    assert_eq!(failed, BTreeSet::from([device(1)]));
    assert!(
        !is_removed(&identity, device(1)),
        "not tombstoned without its rotation"
    );
    round(&identity, &mut failed);
    assert_eq!(failed.len(), 1, "the same peer is not booked twice");
}

#[test]
fn a_peer_with_a_session_open_is_heard_now_however_old_its_last_hello() {
    let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
    let identity = identity_in(dir.path(), [7; 32]);
    register(&identity, 1, NOW - 20 * DAY_MS);
    let live = identity.live_peers().enter(device(1), Carrier::Lan);
    round(&identity, &mut BTreeSet::new());
    assert!(!is_removed(&identity, device(1)), "connected, so not quiet");
    let seen = identity
        .store()
        .lock()
        .unwrap()
        .device(device(1))
        .unwrap()
        .unwrap()
        .last_seen_ms;
    assert_eq!(seen, Some(NOW), "and it counts from now on");
    drop(live);
    round(&identity, &mut BTreeSet::new());
    assert!(!is_removed(&identity, device(1)), "the session just ended");
}
