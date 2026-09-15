//! `ControlMessage` round-trips (task `daemon-workspace-identity-agreement` stage 3): plain
//! encode/decode for every variant, the name-length cap, and a seal/open round trip reusing
//! `aead_tests.rs`'s own fixture shape.

use txtodo_model::{DeviceId, Ulid};

use crate::aead::{GroupKey, GroupKeys};
use crate::control::{
    ControlMessage, ControlMessageError, MAX_WORKSPACE_NAME_BYTES, open_control, seal_control,
};
use crate::message::GroupId;

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
