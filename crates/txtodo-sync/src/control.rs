//! `ControlMessage`: the always-on per-device-set control channel's own message set (task
//! `daemon-workspace-identity-agreement` stage 3) — a sibling to [`crate::message::Message`],
//! never a new variant on it. `Message`'s four variants exist to drive `Session`'s
//! `Idle → Greeted → Wanting → Importing` state machine; a workspace offer has nothing to do with
//! that and folding it in would force every `Session::on_hello`/`on_ops` match arm to reason about
//! a case it can never actually receive. This mirrors an existing precedent in this crate:
//! `pairing_relay.rs`'s `JoinerHello`/`InitiatorReply` are already a second, purpose-built message
//! set, carried on their own ALPN inside the same `Frame` envelope `Message` uses — not folded in
//! either, for the same reason.
//!
//! Sealed with the same group-key AEAD primitives `sealed_ops.rs` already uses for `Message`
//! ([`seal_control`]/[`open_control`] below) — offers only make sense between two already-paired
//! devices, so the group key already exists by the time one is meaningful; inventing a second,
//! lighter-weight seal for "small frames" would be new, unaudited crypto surface for no real gain.
//!
//! `workspace_id` travels as a bare `u128` (a `txtodo_store::WorkspaceId`'s `Ulid::to_u128()`),
//! the same wire shape [`crate::message::GroupId`] already uses for a `txtodo-store`-adjacent
//! identity — `txtodo-store` has no `serde` dependency today, so this crate cannot derive
//! `Serialize` on `WorkspaceId` itself without adding one; the daemon converts at its own boundary,
//! the same idiom `crates/txtodo-daemon/src/global_service.rs::parse_workspace_id` already uses.

use std::fmt;

use serde::{Deserialize, Serialize};
use txtodo_model::DeviceId;

use crate::aead::{GroupKey, GroupKeys, open as aead_open, seal as aead_seal};
use crate::crypto_error::CryptoError;
use crate::frame::{Frame, FrameError, PROTOCOL_VERSION};
use crate::message::GroupId;

/// Most bytes a workspace's display name may carry — plenty for any real directory name, small
/// enough that a hostile peer cannot use it to smuggle an oversized allocation (every wire
/// collection in this crate has a named, checked cap; a bare `String` needs one too).
pub const MAX_WORKSPACE_NAME_BYTES: usize = 256;

/// One control-channel message. Append-only variants, the same postcard-tagged-enum discipline as
/// `Message` (see that type's own doc for why: postcard tags a variant by index, so inserting or
/// reordering one renumbers every variant after it).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ControlMessage {
    /// Announces a workspace the sender has registered — first-registrant-wins (this task's
    /// `notes.md`): the receiver either already knows this id (nothing to do) or offers it to its
    /// own human as a pending accept (`workspace_offer_registry.rs`, stage 4).
    Offer {
        /// The offering device — every control message self-declares its sender (the same
        /// solved problem `Message::Hello.device` already has, since a device-level control
        /// channel talks to potentially many peers and an accepted connection has no other way
        /// to attribute who sent it without exposing `iroh` connection identity across the
        /// crate boundary).
        sender: DeviceId,
        /// The offering device's own `WorkspaceId`, as `Ulid::to_u128()` — see the module doc for
        /// why this is a bare integer rather than the typed `txtodo_store::WorkspaceId`.
        workspace_id: u128,
        /// Human-readable name, for the accept-side prompt only — never used as identity.
        name: String,
        /// The offering device's own clock reading, milliseconds.
        offered_at_ms: u64,
    },
    /// The receiver adopted `workspace_id` verbatim (`WorkspaceRegistry::adopt`, stage 4) into its
    /// own registry.
    OfferAck {
        /// The accepting device — see `Offer.sender`'s doc; here it's whoever is acknowledging,
        /// not whoever originally offered.
        sender: DeviceId,
        /// Echoes the offer's id.
        workspace_id: u128,
    },
    /// The receiver declined the offer. What the offering device does with this (e.g. stop
    /// re-announcing to this peer) is stage 5's bookkeeping, not this type's concern.
    Decline {
        /// The accepting device — see `OfferAck.sender`'s doc.
        sender: DeviceId,
        /// Echoes the offer's id.
        workspace_id: u128,
    },
}

/// Why a `ControlMessage` could not be encoded or decoded. Mirrors [`crate::message::MessageError`]
/// minus the range/signature/heads caps `Message` alone needs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ControlMessageError {
    /// The envelope was wrong (bad magic, unknown version, too large, truncated).
    Frame(FrameError),
    /// `WorkspaceOffer.name` exceeds [`MAX_WORKSPACE_NAME_BYTES`].
    NameTooLong {
        /// Its length.
        len: usize,
        /// The cap.
        max: usize,
    },
    /// postcard could not decode the body as this version's `ControlMessage`.
    Codec(postcard::Error),
    /// The body decoded but bytes were left over — a struct edit or a foreign message.
    TrailingBytes(usize),
}

impl fmt::Display for ControlMessageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ControlMessageError::Frame(e) => write!(f, "frame: {e}"),
            ControlMessageError::NameTooLong { len, max } => {
                write!(f, "workspace name: {len} bytes exceeds the cap of {max}")
            }
            ControlMessageError::Codec(e) => write!(f, "postcard: {e}"),
            ControlMessageError::TrailingBytes(n) => {
                write!(f, "{n} bytes left after the message")
            }
        }
    }
}

impl std::error::Error for ControlMessageError {}

impl From<FrameError> for ControlMessageError {
    fn from(e: FrameError) -> ControlMessageError {
        ControlMessageError::Frame(e)
    }
}

impl ControlMessage {
    /// Encodes into a `Frame` for `PROTOCOL_VERSION`, after checking every cap.
    pub fn encode(&self) -> Result<Frame, ControlMessageError> {
        self.check_caps()?;
        let body = postcard::to_allocvec(self).map_err(ControlMessageError::Codec)?;
        debug_assert!(!body.is_empty(), "every variant encodes at least its tag");
        let frame = Frame::new(body)?;
        debug_assert_eq!(frame.version, PROTOCOL_VERSION);
        Ok(frame)
    }

    /// Decodes a `Frame` body, then checks every cap. A frame for another version is refused.
    pub fn decode(frame: &Frame) -> Result<ControlMessage, ControlMessageError> {
        if frame.version != PROTOCOL_VERSION {
            return Err(FrameError::UnknownVersion {
                got: frame.version,
                supported: PROTOCOL_VERSION,
            }
            .into());
        }
        let (message, rest): (ControlMessage, &[u8]) =
            postcard::take_from_bytes(&frame.body).map_err(ControlMessageError::Codec)?;
        if !rest.is_empty() {
            return Err(ControlMessageError::TrailingBytes(rest.len()));
        }
        message.check_caps()?;
        debug_assert!(rest.is_empty());
        Ok(message)
    }

    /// The one cap this message set needs. Exhaustive over the variants.
    fn check_caps(&self) -> Result<(), ControlMessageError> {
        if let ControlMessage::Offer { name, .. } = self
            && name.len() > MAX_WORKSPACE_NAME_BYTES
        {
            return Err(ControlMessageError::NameTooLong {
                len: name.len(),
                max: MAX_WORKSPACE_NAME_BYTES,
            });
        }
        Ok(())
    }
}

/// Why a sealed `ControlMessage` could not be built or opened. Same split as
/// [`crate::sealed_ops::SealedOpsError`]: the crypto layer and the message layer underneath, one
/// type so a caller matches one `Result` instead of two.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ControlSealError {
    /// A signature could not be produced/verified, or the AEAD seal/open failed.
    Crypto(CryptoError),
    /// The plaintext did not encode/decode as a `ControlMessage`, or failed a cap.
    Message(ControlMessageError),
}

impl fmt::Display for ControlSealError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ControlSealError::Crypto(e) => write!(f, "{e}"),
            ControlSealError::Message(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for ControlSealError {}

impl From<CryptoError> for ControlSealError {
    fn from(e: CryptoError) -> ControlSealError {
        ControlSealError::Crypto(e)
    }
}

impl From<ControlMessageError> for ControlSealError {
    fn from(e: ControlMessageError) -> ControlSealError {
        ControlSealError::Message(e)
    }
}

/// Seals `msg` for `group` under `epoch`'s key — the control-channel twin of
/// [`crate::sealed_ops::seal_ops`], minus the per-op signing a `ControlMessage` has no ops to need.
pub fn seal_control(
    msg: &ControlMessage,
    group: GroupId,
    epoch: u32,
    key: &GroupKey,
) -> Result<Frame, ControlSealError> {
    let frame = msg.encode()?;
    let sealed = aead_seal(PROTOCOL_VERSION, group, epoch, key, &frame.body)?;
    Ok(Frame {
        version: PROTOCOL_VERSION,
        body: sealed,
    })
}

/// Opens `frame` with `group_keys` — a wrong or unretained group key is refused here, before a
/// single byte of `ControlMessage` is parsed.
pub fn open_control(
    frame: &Frame,
    group: GroupId,
    group_keys: &GroupKeys,
) -> Result<ControlMessage, ControlSealError> {
    let plaintext = aead_open(PROTOCOL_VERSION, group, group_keys, &frame.body)?;
    Ok(ControlMessage::decode(&Frame {
        version: PROTOCOL_VERSION,
        body: plaintext,
    })?)
}
