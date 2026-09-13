//! Wire messages for the pairing handshake's daemon-to-daemon leg (plan M4 `sync-pairing`, LAN
//! wiring pass): the bytes [`crate::pairing::PairingSession`]'s crypto needs, carried over a
//! [`crate::link::Link`] on its own connection — never the group-keyed [`crate::message::Message`]/
//! [`crate::session::Session`] protocol, since the two daemons pairing do not yet share a group key
//! (that is the whole point of pairing). Frames still use this crate's one [`crate::frame::Frame`]
//! envelope (`Frame::new`/`Frame::decode`, `PROTOCOL_VERSION`); the separation from `Message` is at
//! a different layer entirely (a distinct iroh ALPN, `crate::endpoint::PAIRING_ALPN`, checked via
//! `crate::lan_link::IrohLink::alpn`), not a different frame version, so nothing here touches
//! `frame.rs`'s frozen layout.
//!
//! Nothing serialised here is more secret than what already crosses in the QR/`PairOffer` payload
//! (an ephemeral or long-term *public* key) except [`InitiatorReply::Grant`]'s sealed bytes, which
//! are already AEAD-sealed by [`crate::pairing::PairingSession::wrap_grant`] under a key neither
//! device had before this handshake's SAS-bound transcript — this module carries that ciphertext,
//! it does not add or remove any protection of its own. Ref: <https://docs.rs/postcard>.

use serde::{Deserialize, Serialize};
use txtodo_model::DeviceId;

use crate::device_static::DEVICE_STATIC_KEY_BYTES;
use crate::frame::{Frame, FrameError, PROTOCOL_VERSION};
use crate::message::GroupId;
use crate::nonce_registry::Nonce;
use crate::transcript::X25519_PUBLIC_KEY_BYTES;

/// Joiner -> initiator, one per connection attempt. Self-sufficient and idempotent: a retried
/// connection (after a dropped reply, or while the humans are still comparing words) resends the
/// same identity and keys, plus whatever `confirmed` is true *as of this attempt* — the initiator
/// applies it idempotently (`PairingSession::confirm_remote` is safe to call more than once).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct JoinerHello {
    /// The joiner's own device id.
    pub device: DeviceId,
    /// The group the joiner is trying to join — must match the initiator's own active offer, or
    /// this is a stale or foreign attempt.
    pub group: GroupId,
    /// The offer's one-time nonce, echoed back so the initiator can confirm this is answering the
    /// same offer it handed out, not a leftover retry against a since-reissued one.
    pub nonce: Nonce,
    /// The joiner's fresh ephemeral X25519 public key (`PairingSession::accept`'s own output).
    pub public_key: [u8; X25519_PUBLIC_KEY_BYTES],
    /// The joiner's long-term static public key (plan M4 `sync-device-remove`), so the initiator
    /// can register it symmetrically with the joiner registering the initiator's own (carried
    /// inside `PairingGrant`) — sent in the clear like every other public key here.
    pub static_public: [u8; DEVICE_STATIC_KEY_BYTES],
    /// Whether this device's own human has confirmed the SAS as of this attempt.
    pub confirmed: bool,
}

/// Initiator -> joiner, one reply per [`JoinerHello`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum InitiatorReply {
    /// Recorded; not both sides have confirmed yet. Keep retrying.
    Pending,
    /// The nonce or group did not match this daemon's active pairing attempt (stale, foreign, or
    /// no pairing active at all). Stop retrying.
    Rejected,
    /// Both sides confirmed: the sealed [`crate::pairing_grant::PairingGrant`]
    /// (`PairingSession::wrap_grant`'s output) — open with
    /// [`crate::pairing::PairingSession::unwrap_grant`].
    Grant(Vec<u8>),
}

/// Why a [`JoinerHello`]/[`InitiatorReply`] frame did not decode. Mirrors
/// [`crate::message::MessageError`]'s shape for this much smaller wire vocabulary.
#[derive(Debug)]
pub enum PairingRelayError {
    /// The outer frame envelope failed (bad magic, truncated, over-cap, unknown version).
    Frame(FrameError),
    /// The frame decoded, but its body was not the shape expected.
    Codec(postcard::Error),
    /// Bytes remained after decoding the message — the frame carried more than one value.
    TrailingBytes(usize),
}

impl std::fmt::Display for PairingRelayError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PairingRelayError::Frame(e) => write!(f, "frame: {e}"),
            PairingRelayError::Codec(e) => write!(f, "postcard: {e}"),
            PairingRelayError::TrailingBytes(n) => write!(f, "{n} bytes left after the message"),
        }
    }
}

impl std::error::Error for PairingRelayError {}

impl From<FrameError> for PairingRelayError {
    fn from(e: FrameError) -> PairingRelayError {
        PairingRelayError::Frame(e)
    }
}

fn encode<T: Serialize>(msg: &T) -> Result<Frame, PairingRelayError> {
    let body = postcard::to_allocvec(msg).map_err(PairingRelayError::Codec)?;
    Ok(Frame::new(body)?)
}

fn decode<T: for<'a> Deserialize<'a>>(frame: &Frame) -> Result<T, PairingRelayError> {
    if frame.version != PROTOCOL_VERSION {
        return Err(FrameError::UnknownVersion {
            got: frame.version,
            supported: PROTOCOL_VERSION,
        }
        .into());
    }
    let (msg, rest): (T, &[u8]) =
        postcard::take_from_bytes(&frame.body).map_err(PairingRelayError::Codec)?;
    if !rest.is_empty() {
        return Err(PairingRelayError::TrailingBytes(rest.len()));
    }
    Ok(msg)
}

impl JoinerHello {
    /// Encodes this message as a [`Frame`] ready for [`crate::link::Link::send`].
    pub fn encode(&self) -> Result<Frame, PairingRelayError> {
        encode(self)
    }

    /// Decodes a [`Frame`] received from [`crate::link::Link::recv`].
    pub fn decode(frame: &Frame) -> Result<JoinerHello, PairingRelayError> {
        decode(frame)
    }
}

impl InitiatorReply {
    /// Encodes this message as a [`Frame`] ready for [`crate::link::Link::send`].
    pub fn encode(&self) -> Result<Frame, PairingRelayError> {
        encode(self)
    }

    /// Decodes a [`Frame`] received from [`crate::link::Link::recv`].
    pub fn decode(frame: &Frame) -> Result<InitiatorReply, PairingRelayError> {
        decode(frame)
    }
}
