//! The pairing transcript: `protocol_version || device_a || pub_a || device_b || pub_b || group_id`.
//! Both devices must compute byte-identical bytes without agreeing out-of-band on who is "a" — so
//! `a`/`b` here is a canonical order (ascending `DeviceId`), not initiator/joiner. Binding both
//! devices' identities, both ephemeral public keys, the group and the protocol version means an
//! active machine-in-the-middle running two independent handshakes cannot make both victims land on
//! the same transcript, which is what makes the derived SAS ([`crate::sas`]) mean anything at all.

use txtodo_model::DeviceId;

use crate::message::GroupId;

/// Bytes in one X25519 public key.
pub const X25519_PUBLIC_KEY_BYTES: usize = 32;
/// Total transcript length: `u16 + (16 + 32) * 2 + 16`.
pub const TRANSCRIPT_BYTES: usize = 2 + (16 + X25519_PUBLIC_KEY_BYTES) * 2 + 16;

/// One side of a pairing handshake: its device id and its ephemeral public key. Bundled so
/// [`transcript`] stays within this workspace's argument-count limit and so a caller cannot
/// transpose a device id with the wrong public key by mixing up positional arguments.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Party {
    /// The device id.
    pub device: DeviceId,
    /// Its ephemeral X25519 public key for this handshake.
    pub public_key: [u8; X25519_PUBLIC_KEY_BYTES],
}

/// Builds the transcript for one pairing handshake between `party1` and `party2` under `group`,
/// speaking `protocol_version`. The two parties are reordered by ascending `DeviceId` so either
/// side calling this with its own view of "self" and "peer" produces the same bytes as the other
/// side calling it with the roles reversed.
pub fn transcript(
    protocol_version: u16,
    party1: Party,
    party2: Party,
    group: GroupId,
) -> [u8; TRANSCRIPT_BYTES] {
    let (a, b) = if party1.device <= party2.device {
        (party1, party2)
    } else {
        (party2, party1)
    };
    let mut out = [0u8; TRANSCRIPT_BYTES];
    let mut at = 0;
    out[at..at + 2].copy_from_slice(&protocol_version.to_le_bytes());
    at += 2;
    out[at..at + 16].copy_from_slice(&a.device.ulid().to_u128().to_le_bytes());
    at += 16;
    out[at..at + X25519_PUBLIC_KEY_BYTES].copy_from_slice(&a.public_key);
    at += X25519_PUBLIC_KEY_BYTES;
    out[at..at + 16].copy_from_slice(&b.device.ulid().to_u128().to_le_bytes());
    at += 16;
    out[at..at + X25519_PUBLIC_KEY_BYTES].copy_from_slice(&b.public_key);
    at += X25519_PUBLIC_KEY_BYTES;
    out[at..at + 16].copy_from_slice(&group.0.to_le_bytes());
    at += 16;
    debug_assert_eq!(at, TRANSCRIPT_BYTES);
    out
}
