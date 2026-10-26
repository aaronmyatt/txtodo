//! `ControlMessage` round-trips (task `daemon-workspace-identity-agreement` stage 3): plain
//! encode/decode for every variant, the name-length cap, and a seal/open round trip reusing
//! `aead_tests.rs`'s own fixture shape.

use txtodo_model::{DeviceId, Ulid};

use crate::aead::{GroupKey, GroupKeys};
use crate::control::{
    ControlMessage, ControlMessageError, MAX_GRANT_SEALED_BYTES, MAX_OWN_DEVICES,
    MAX_WORKSPACE_NAME_BYTES, open_control, seal_control,
};
use crate::message::GroupId;
use crate::rotation::WrappedGrant;

fn key(byte: u8) -> GroupKey {
    GroupKey::from_bytes([byte; 32])
}

fn one_epoch(epoch: u32, key_byte: u8) -> GroupKeys {
    let mut keys = GroupKeys::new();
    keys.insert(epoch, key(key_byte)).unwrap();
    keys
}

fn device(n: u128) -> DeviceId {
    DeviceId::new(Ulid::from_u128(n))
}

fn wrapped(epoch: u32) -> WrappedGrant {
    WrappedGrant {
        epoch,
        ephemeral_public: [8; 32],
        sealed: vec![3; 72],
    }
}

fn a_grant(left_out: Vec<DeviceId>) -> ControlMessage {
    ControlMessage::grant(device(1), device(2), wrapped(4), left_out)
}

fn offer() -> ControlMessage {
    ControlMessage::Offer {
        sender: device(1),
        workspace_id: 0x1234_5678_9ABC_DEF0,
        name: "my-project".to_string(),
        offered_at_ms: 1_000,
    }
}

#[test]
fn every_variant_round_trips_through_encode_decode() {
    let variants = [
        offer(),
        ControlMessage::OfferAck {
            sender: device(2),
            workspace_id: 42,
        },
        ControlMessage::Decline {
            sender: device(2),
            workspace_id: 42,
        },
        ControlMessage::OwnDevices {
            sender: device(1),
            devices: vec![device(3), device(4)],
        },
        a_grant(vec![device(5)]),
        ControlMessage::Epoch {
            sender: device(6),
            epoch: 7,
        },
    ];
    for msg in variants {
        let frame = msg.encode().unwrap();
        assert_eq!(ControlMessage::decode(&frame).unwrap(), msg);
    }
}

#[test]
fn a_name_over_the_cap_is_refused_before_encoding_grows_unbounded() {
    let msg = ControlMessage::Offer {
        sender: device(1),
        workspace_id: 1,
        name: "x".repeat(MAX_WORKSPACE_NAME_BYTES + 1),
        offered_at_ms: 0,
    };
    match msg.encode() {
        Err(ControlMessageError::NameTooLong { len, max }) => {
            assert_eq!(len, MAX_WORKSPACE_NAME_BYTES + 1);
            assert_eq!(max, MAX_WORKSPACE_NAME_BYTES);
        }
        other => panic!("expected NameTooLong, got {other:?}"),
    }
}

/// ADR 0029's amendment: a vouch list is bounded like any other wire collection.
#[test]
fn an_own_devices_list_over_the_cap_is_refused_and_one_at_it_is_not() {
    let list = |n: usize| ControlMessage::OwnDevices {
        sender: device(1),
        devices: (0..n as u128).map(device).collect(),
    };
    match list(MAX_OWN_DEVICES + 1).encode() {
        Err(ControlMessageError::TooManyDevices { len, max }) => {
            assert_eq!((len, max), (MAX_OWN_DEVICES + 1, MAX_OWN_DEVICES));
        }
        other => panic!("expected TooManyDevices, got {other:?}"),
    }
    assert!(list(MAX_OWN_DEVICES).encode().is_ok());
}

#[test]
fn a_name_at_the_cap_is_accepted() {
    let msg = ControlMessage::Offer {
        sender: device(1),
        workspace_id: 1,
        name: "x".repeat(MAX_WORKSPACE_NAME_BYTES),
        offered_at_ms: 0,
    };
    assert!(msg.encode().is_ok());
}

#[test]
fn seal_and_open_round_trip_with_the_group_key() {
    let group = GroupId(7);
    let keys = one_epoch(0, 9);
    let msg = offer();

    let sealed = seal_control(&msg, group, 0, keys.get(0).unwrap()).unwrap();
    let opened = open_control(&sealed, group, &keys).unwrap();
    assert_eq!(opened, msg);
}

#[test]
fn opening_with_the_wrong_group_key_fails() {
    let group = GroupId(7);
    let keys = one_epoch(0, 9);
    let wrong_keys = one_epoch(0, 200);
    let sealed = seal_control(&offer(), group, 0, keys.get(0).unwrap()).unwrap();
    assert!(open_control(&sealed, group, &wrong_keys).is_err());
}

#[test]
fn a_grant_carries_its_wrapped_key_back_out_and_only_a_grant_does() {
    let msg = a_grant(vec![device(9)]);
    assert_eq!(msg.wrapped_grant(), Some(wrapped(4)));
    assert!(offer().wrapped_grant().is_none());
    match msg {
        ControlMessage::Grant {
            sender,
            minter,
            epoch,
            left_out,
            ..
        } => {
            assert_eq!((sender, minter, epoch), (device(1), device(2), 4));
            assert_eq!(left_out, vec![device(9)]);
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_grant_is_capped_in_its_sealed_bytes_and_the_devices_it_names() {
    let big = ControlMessage::grant(
        device(1),
        device(2),
        WrappedGrant {
            sealed: vec![0; MAX_GRANT_SEALED_BYTES + 1],
            ..wrapped(1)
        },
        vec![],
    );
    assert_eq!(
        big.encode().unwrap_err(),
        ControlMessageError::GrantTooLong {
            len: MAX_GRANT_SEALED_BYTES + 1,
            max: MAX_GRANT_SEALED_BYTES
        }
    );
    let many = a_grant((0..=MAX_OWN_DEVICES as u128).map(device).collect());
    assert!(matches!(
        many.encode().unwrap_err(),
        ControlMessageError::TooManyDevices { .. }
    ));
}

#[test]
fn a_grant_sealed_under_the_previous_epoch_opens_with_that_epoch_and_names_it() {
    // The new epoch's key cannot seal its own grant: the receiver has not got it yet.
    let (group, msg) = (GroupId(5), a_grant(vec![device(7)]));
    let frame = seal_control(&msg, group, 3, &key(1)).unwrap();
    assert_eq!(crate::aead::sealed_epoch(&frame.body), Some(3));
    let mut held = one_epoch(3, 1);
    held.insert(2, key(9)).unwrap();
    assert_eq!(open_control(&frame, group, &held).unwrap(), msg);
}
