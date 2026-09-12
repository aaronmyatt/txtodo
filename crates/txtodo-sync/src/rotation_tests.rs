//! Rotation grants: round-trip, wrong recipient fails, tampering fails, the removed device gets
//! no grant, and the removal guards (never self, never the last device).

use std::collections::BTreeMap;

use txtodo_model::{DeviceId, Ulid};

use crate::device_static::DeviceStaticSecret;
use crate::rotation::{open_grant, plan_rotation, validate_removal, wrap_grant_for};
use crate::rotation_error::{RemovalError, RotationError};

fn dev(n: u128) -> DeviceId {
    DeviceId::new(Ulid::from_u128(n))
}

#[test]
fn wrap_then_open_recovers_the_exact_key_bytes() {
    let recipient = DeviceStaticSecret::generate();
    let new_key = [7u8; 32];
    let grant = wrap_grant_for(&new_key, 4, &recipient.public_key()).unwrap();
    assert_eq!(grant.epoch, 4);
    assert_eq!(open_grant(&grant, &recipient).unwrap(), new_key);
}

#[test]
fn a_different_devices_secret_cannot_open_the_grant() {
    let recipient = DeviceStaticSecret::generate();
    let stranger = DeviceStaticSecret::generate();
    let grant = wrap_grant_for(&[1u8; 32], 1, &recipient.public_key()).unwrap();
    assert_eq!(
        open_grant(&grant, &stranger).unwrap_err(),
        RotationError::Open
    );
}

#[test]
fn a_tampered_grant_fails_to_open() {
    let recipient = DeviceStaticSecret::generate();
    let mut grant = wrap_grant_for(&[1u8; 32], 1, &recipient.public_key()).unwrap();
    let last = grant.sealed.len() - 1;
    grant.sealed[last] ^= 1;
    assert_eq!(
        open_grant(&grant, &recipient).unwrap_err(),
        RotationError::Open
    );
}

#[test]
fn a_grant_for_one_epoch_does_not_open_relabelled_as_another() {
    let recipient = DeviceStaticSecret::generate();
    let mut grant = wrap_grant_for(&[1u8; 32], 1, &recipient.public_key()).unwrap();
    grant.epoch = 2;
    assert_eq!(
        open_grant(&grant, &recipient).unwrap_err(),
        RotationError::Open
    );
}

#[test]
fn plan_rotation_grants_every_remaining_device_and_advances_the_epoch() {
    let a = DeviceStaticSecret::generate();
    let b = DeviceStaticSecret::generate();
    let mut remaining = BTreeMap::new();
    remaining.insert(dev(1), a.public_key());
    remaining.insert(dev(2), b.public_key());
    let new_key = [9u8; 32];
    let grants = plan_rotation(4, &new_key, &remaining).unwrap();
    assert_eq!(grants.len(), 2);
    assert_eq!(grants[&dev(1)].epoch, 5);
    assert_eq!(open_grant(&grants[&dev(1)], &a).unwrap(), new_key);
    assert_eq!(open_grant(&grants[&dev(2)], &b).unwrap(), new_key);
}

#[test]
fn the_removed_device_receives_no_grant() {
    let a = DeviceStaticSecret::generate();
    let removed = DeviceStaticSecret::generate();
    let mut remaining = BTreeMap::new();
    remaining.insert(dev(1), a.public_key());
    // `removed`'s device id is simply absent from `remaining` — assert on the grant set, not on a
    // failed decrypt, per the task notes.
    let grants = plan_rotation(0, &[3u8; 32], &remaining).unwrap();
    assert_eq!(grants.len(), 1);
    assert!(grants.contains_key(&dev(1)));
    let _ = removed; // held only to make "removed" concrete in the scenario, never granted to
}

#[test]
fn plan_rotation_refuses_an_empty_remaining_set() {
    let grants = plan_rotation(0, &[1u8; 32], &BTreeMap::new());
    assert_eq!(grants.unwrap_err(), RotationError::NoRemainingDevices);
}

#[test]
fn removing_yourself_removing_the_last_device_and_a_normal_removal_are_distinct() {
    assert_eq!(
        validate_removal(dev(1), dev(1), 3),
        Err(RemovalError::CannotRemoveSelf { device: dev(1) })
    );
    assert_eq!(
        validate_removal(dev(2), dev(1), 1),
        Err(RemovalError::CannotRemoveLastDevice)
    );
    assert_eq!(validate_removal(dev(2), dev(1), 3), Ok(()));
}
