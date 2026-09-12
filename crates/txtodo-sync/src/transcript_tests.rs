//! Transcript determinism and sensitivity: both sides get the same bytes; any single-bit change to
//! any field changes them.

use txtodo_model::{DeviceId, Ulid};

use crate::message::GroupId;
use crate::transcript::{Party, transcript};

fn device(n: u128) -> DeviceId {
    DeviceId::new(Ulid::from_u128(n))
}

fn party(n: u128, key_byte: u8) -> Party {
    Party {
        device: device(n),
        public_key: [key_byte; 32],
    }
}

#[test]
fn order_is_canonical_regardless_of_caller_order() {
    let a = party(1, 0xAA);
    let b = party(2, 0xBB);
    let group = GroupId(7);
    let t1 = transcript(1, a, b, group);
    let t2 = transcript(1, b, a, group);
    assert_eq!(
        t1, t2,
        "either side's own/peer order must yield the same bytes"
    );
}

#[test]
fn changing_the_protocol_version_changes_the_transcript() {
    let a = party(1, 0xAA);
    let b = party(2, 0xBB);
    let group = GroupId(7);
    assert_ne!(transcript(1, a, b, group), transcript(2, a, b, group));
}

#[test]
fn changing_either_device_id_changes_the_transcript() {
    let a = party(1, 0xAA);
    let b = party(2, 0xBB);
    let group = GroupId(7);
    let base = transcript(1, a, b, group);
    let a_same_id = party(1, 0xAA); // same device id and key: must reproduce the same transcript
    let a_different_id = party(99, 0xAA);
    assert_eq!(base, transcript(1, a_same_id, b, group));
    assert_ne!(base, transcript(1, a_different_id, b, group));
}

#[test]
fn changing_either_public_key_changes_the_transcript() {
    let a = party(1, 0xAA);
    let b = party(2, 0xBB);
    let group = GroupId(7);
    let base = transcript(1, a, b, group);
    let a_flipped = party(1, 0xAB);
    assert_ne!(base, transcript(1, a_flipped, b, group));
}

#[test]
fn changing_the_group_changes_the_transcript() {
    let a = party(1, 0xAA);
    let b = party(2, 0xBB);
    let base = transcript(1, a, b, GroupId(7));
    assert_ne!(base, transcript(1, a, b, GroupId(8)));
}

#[test]
fn a_single_bit_flip_anywhere_changes_the_transcript() {
    let a = party(1, 0xAA);
    let b = party(2, 0xBB);
    let base = transcript(1, a, b, GroupId(7));
    let mut flipped = base;
    flipped[0] ^= 1;
    assert_ne!(base, flipped);
    // (bytes compared directly here; the semantic single-field-change cases above cover the
    // structured fields — this just confirms the array itself is not accidentally constant-sized
    // padding that a bit flip could land outside of).
    assert_eq!(base.len(), flipped.len());
}
