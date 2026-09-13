//! Round-trip and error-shape coverage for `pairing_relay.rs`'s wire messages.

use txtodo_model::{DeviceId, Ulid};

use crate::device_static::DEVICE_STATIC_KEY_BYTES;
use crate::frame::Frame;
use crate::message::GroupId;
use crate::pairing_relay::{InitiatorReply, JoinerHello, PairingRelayError};
use crate::transcript::X25519_PUBLIC_KEY_BYTES;

fn sample_hello() -> JoinerHello {
    JoinerHello {
        device: DeviceId::new(Ulid::from_u128(42)),
        group: GroupId(7),
        nonce: [9u8; 16],
        public_key: [1u8; X25519_PUBLIC_KEY_BYTES],
        static_public: [2u8; DEVICE_STATIC_KEY_BYTES],
        confirmed: false,
    }
}

#[test]
fn joiner_hello_round_trips() {
    let hello = sample_hello();
    let frame = hello.encode().expect("encode");
    let decoded = JoinerHello::decode(&frame).expect("decode");
    assert_eq!(hello, decoded);
}

#[test]
fn initiator_reply_round_trips_every_variant() {
    for reply in [
        InitiatorReply::Pending,
        InitiatorReply::Rejected,
        InitiatorReply::Grant(vec![1, 2, 3]),
    ] {
        let frame = reply.encode().expect("encode");
        let decoded = InitiatorReply::decode(&frame).expect("decode");
        assert_eq!(reply, decoded);
    }
}

#[test]
fn decoding_the_wrong_message_type_is_a_typed_error_not_a_panic() {
    let frame = InitiatorReply::Pending.encode().expect("encode");
    let err = JoinerHello::decode(&frame).expect_err("wrong shape");
    assert!(matches!(err, PairingRelayError::Codec(_)));
}

#[test]
fn decoding_a_foreign_frame_version_is_refused() {
    let frame = Frame {
        version: 0xFFFF,
        body: vec![0],
    };
    let err = JoinerHello::decode(&frame).expect_err("unknown version");
    assert!(matches!(
        err,
        PairingRelayError::Frame(crate::frame::FrameError::UnknownVersion { .. })
    ));
}
