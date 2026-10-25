//! `group_rotation.rs` between real in-memory identities: a rotation on one device, the grant it
//! makes for another, what the receiver does with it, and two devices that rotate at once.

use super::*;
use crate::clock::FakeClock;
use txtodo_model::Ulid;
use txtodo_store::NewDevice;
use txtodo_sync::{KeyId, Secret};

struct Dev {
    _dir: tempfile::TempDir,
    id: DeviceIdentity,
}

/// Device `n`. Its id comes from the clock, so each `n` is another device, and a lower `n` is a lower
/// id (the tie-break).
fn dev(n: u64) -> Dev {
    let dir = tempfile::tempdir().unwrap();
    let id = DeviceIdentity::open_in_memory(dir.path(), &FakeClock::new(1_000 * n)).unwrap();
    // The group's first key, the same on every device.
    id.key_store()
        .put(KeyId::Group(0), &Secret::new(vec![5; 32]))
        .unwrap();
    Dev { _dir: dir, id }
}

/// `holder` knows `peer` (its device id and static key) as a paired, own device.
fn know(holder: &DeviceIdentity, peer: DeviceId, static_public: [u8; 32]) {
    let new = NewDevice {
        device: peer,
        name: String::new(),
        static_public,
        paired_at_ms: 500,
        last_known_wall_ms: None,
        key_epoch: 0,
    };
    holder
        .store()
        .lock()
        .unwrap()
        .register_device_as(&new, true)
        .unwrap();
}

fn know_each_other(a: &Dev, b: &Dev) {
    know(&a.id, b.id.device(), b.id.device_static_public().to_bytes());
    know(&b.id, a.id.device(), a.id.device_static_public().to_bytes());
}

/// A device that is only a row: some other device's id with a static key nobody holds.
fn ghost(n: u128) -> DeviceId {
    DeviceId::new(Ulid::from_u128(
        0x0190_0000_0000_0000_0000_0000_0000_0000 + n,
    ))
}

fn removed(dev: &Dev, peer: DeviceId) -> bool {
    dev.id
        .store()
        .lock()
        .unwrap()
        .device(peer)
        .unwrap()
        .is_some_and(|row| row.removed_at_ms.is_some())
}

fn key_at(dev: &Dev, epoch: u32) -> Option<Vec<u8>> {
    let held = dev.id.key_store().get(KeyId::Group(epoch)).unwrap();
    held.map(|s| s.expose().to_vec())
}

/// `a` and `b` know each other and both know `ghost(1)`; `a` removes the ghost and rotates to 1.
fn a_rotates_the_ghost_out() -> (Dev, Dev) {
    let (a, b) = (dev(1), dev(2));
    know_each_other(&a, &b);
    for d in [&a, &b] {
        know(&d.id, ghost(1), [3; 32]);
    }
    let out = a.id.remove_device(ghost(1), 2_000).unwrap();
    assert_eq!(out.rotated_to_epoch, 1);
    (a, b)
}

#[test]
fn a_kept_peer_is_granted_the_new_key_and_the_dropped_one_is_not() {
    let (a, b) = a_rotates_the_ghost_out();
    let grants = grants_for(&a.id, b.id.device(), 0);
    assert_eq!(grants.len(), 1);
    assert_eq!(
        grants[0].sealed_under, 0,
        "sealed under what the peer holds"
    );
    match &grants[0].message {
        ControlMessage::Grant {
            sender,
            minter,
            epoch,
            left_out,
            ..
        } => {
            assert_eq!(
                (*sender, *minter, *epoch),
                (a.id.device(), a.id.device(), 1)
            );
            assert_eq!(left_out, &vec![ghost(1)]);
        }
        other => panic!("{other:?}"),
    }
    assert!(grants_for(&a.id, b.id.device(), 1).is_empty(), "not behind");
    assert!(
        grants_for(&a.id, ghost(1), 0).is_empty(),
        "the dropped device gets none"
    );
}

#[test]
fn a_grant_moves_the_receiver_to_the_new_epoch_and_drops_whom_it_leaves_out() {
    let (a, b) = a_rotates_the_ghost_out();
    let grant = grants_for(&a.id, b.id.device(), 0).remove(0).message;

    let applied = apply_grant(&b.id, &grant, 3_000);
    assert_eq!(applied, Applied::Adopted { epoch: 1 });
    assert_eq!(b.id.group_epoch(), 1);
    assert_eq!(key_at(&b, 1), key_at(&a, 1), "the same key");
    assert_ne!(key_at(&b, 1), key_at(&b, 0), "and not the old one");
    assert!(removed(&b, ghost(1)), "dropped here too");
    assert_eq!(believed_epoch(&b.id, a.id.device()), Some(1));
    assert_eq!(
        apply_grant(&b.id, &grant, 3_001),
        Applied::Kept,
        "the same grant twice changes nothing"
    );
}

#[test]
fn a_grant_from_a_device_the_receiver_does_not_know_is_ignored() {
    let (a, _b) = a_rotates_the_ghost_out();
    let stranger = dev(4);
    know(
        &a.id,
        stranger.id.device(),
        stranger.id.device_static_public().to_bytes(),
    );
    let grant = grants_for(&a.id, stranger.id.device(), 0).remove(0).message;
    assert_eq!(
        apply_grant(&stranger.id, &grant, 1),
        Applied::Ignored("unknown_sender")
    );
    assert_eq!(stranger.id.group_epoch(), 0);
}

#[test]
fn a_grant_for_someone_else_cannot_be_opened() {
    let (a, b) = a_rotates_the_ghost_out();
    let c = dev(3);
    know_each_other(&b, &c);
    know(&c.id, a.id.device(), a.id.device_static_public().to_bytes());
    let grant = grants_for(&a.id, b.id.device(), 0).remove(0).message;
    // C knows A, but the grant is wrapped to B's key.
    assert_eq!(
        apply_grant(&c.id, &grant, 1),
        Applied::Ignored("cannot_open")
    );
    assert_eq!(c.id.group_epoch(), 0);
}

#[test]
fn the_chain_for_a_peer_two_epochs_behind_is_in_order_each_sealed_under_the_one_before() {
    let (a, b) = a_rotates_the_ghost_out();
    know(&a.id, ghost(2), [4; 32]);
    a.id.remove_device(ghost(2), 2_500).unwrap();
    assert_eq!(a.id.group_epoch(), 2);

    let chain = grants_for(&a.id, b.id.device(), 0);
    let under: Vec<u32> = chain.iter().map(|g| g.sealed_under).collect();
    assert_eq!(under, [0, 1]);
    // B reads them in order and ends on epoch 2, holding both keys.
    for grant in &chain {
        assert!(matches!(
            apply_grant(&b.id, &grant.message, 4_000),
            Applied::Adopted { .. }
        ));
    }
    assert_eq!(b.id.group_epoch(), 2);
    assert_eq!(key_at(&b, 2), key_at(&a, 2));
    assert!(grants_for(&a.id, b.id.device(), 2).is_empty());
}

#[test]
fn a_peer_dropped_in_the_middle_of_the_chain_gets_the_grants_before_it_only() {
    let (a, b) = (dev(1), dev(2));
    know_each_other(&a, &b);
    let c = dev(3);
    know(&a.id, c.id.device(), c.id.device_static_public().to_bytes());
    know(&a.id, ghost(1), [3; 32]);
    a.id.remove_device(ghost(1), 2_000).unwrap(); // epoch 1: c kept
    a.id.remove_device(c.id.device(), 2_500).unwrap(); // epoch 2: c dropped
    let for_c = grants_for(&a.id, c.id.device(), 0);
    // c is removed on `a`, so it gets nothing at all, not even the epoch it was still in.
    assert!(for_c.is_empty());
    assert_eq!(grants_for(&a.id, b.id.device(), 0).len(), 2);
}

#[test]
fn the_lower_minter_wins_when_two_devices_make_a_key_for_one_epoch() {
    let low = DeviceId::new(Ulid::from_u128(1));
    let high = DeviceId::new(Ulid::from_u128(2));
    assert_eq!(
        decide(1, 2, Some(high), low),
        Decision::Adopt,
        "the next epoch"
    );
    assert_eq!(
        decide(1, 1, Some(high), low),
        Decision::Adopt,
        "lower minter wins"
    );
    assert_eq!(decide(1, 1, Some(low), high), Decision::Skip("mine_wins"));
    assert_eq!(decide(1, 1, Some(low), low), Decision::Already);
    assert_eq!(
        decide(1, 1, None, low),
        Decision::Skip("held_without_a_record")
    );
    assert_eq!(decide(2, 1, Some(low), low), Decision::Skip("stale"));
    assert_eq!(decide(1, 3, None, low), Decision::Skip("gap"));
}

#[test]
fn two_devices_that_rotate_at_once_end_on_one_key() {
    let (a, b) = (dev(1), dev(2));
    know_each_other(&a, &b);
    for (d, ghost_n) in [(&a, 1), (&b, 2)] {
        know(&d.id, ghost(1), [3; 32]);
        know(&d.id, ghost(2), [4; 32]);
        d.id.remove_device(ghost(ghost_n), 2_000).unwrap();
    }
    assert_ne!(key_at(&a, 1), key_at(&b, 1), "two keys for epoch 1");

    let from_a = grants_for(&a.id, b.id.device(), 0).remove(0).message;
    let from_b = grants_for(&b.id, a.id.device(), 0).remove(0).message;
    let on_b = apply_grant(&b.id, &from_a, 3_000);
    let on_a = apply_grant(&a.id, &from_b, 3_000);

    assert_eq!(key_at(&a, 1), key_at(&b, 1), "one key now");
    let adopted = [&on_a, &on_b]
        .iter()
        .filter(|r| matches!(r, Applied::Adopted { .. }))
        .count();
    assert_eq!(adopted, 1, "exactly one side gave way: {on_a:?} {on_b:?}");
}

#[test]
fn what_a_peer_is_known_to_hold_only_goes_up() {
    let (a, b) = (dev(1), dev(2));
    know_each_other(&a, &b);
    note_peer_epoch(&a.id, b.id.device(), 3);
    note_peer_epoch(&a.id, b.id.device(), 1);
    assert_eq!(believed_epoch(&a.id, b.id.device()), Some(3));
    assert_eq!(believed_epoch(&a.id, ghost(9)), None);
    assert!(!is_removed(&a.id, b.id.device()));
}
