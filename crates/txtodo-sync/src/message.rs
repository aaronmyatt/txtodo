//! The four protocol messages, postcard-encoded inside a `Frame` body.
//!
//! **Variants are append-only.** postcard tags an enum variant by its index
//! (<https://postcard.jamesmunns.com/wire-format#tagged-unions>), so inserting or reordering a
//! variant renumbers every one after it and old peers decode the wrong message without an error.
//! New messages go at the end; removed ones keep their slot as a tombstone. Every collection on
//! the wire has a named cap, checked before encode and after decode; the body itself is already
//! bounded by `MAX_FRAME_BYTES`, so a hostile count can never allocate past that.

use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};
use txtodo_model::{DeviceId, Op};

use crate::frame::{Frame, FrameError, PROTOCOL_VERSION};

/// Most ops one `Ops` message carries; a longer batch is split, never grown.
pub const MAX_OPS_PER_BATCH: usize = 1_000;
/// Most `(device, range)` entries in one `Want`, `Ops` or `Ack`.
pub const MAX_WANT_RANGES: usize = 1_024;
/// Most devices one `Hello` may report heads for.
pub const MAX_HEADS: usize = 1_024;

/// The sync group two devices must share; the crypto task binds it to the group key.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct GroupId(pub u128);

/// Per origin device, the highest `origin_seq` we hold from it (dense from 1; 0 = none).
pub type Heads = BTreeMap<DeviceId, u64>;

/// An inclusive run of one device's ops by `origin_seq`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct OriginRange {
    /// Whose ops.
    pub device: DeviceId,
    /// First `origin_seq`, inclusive.
    pub first: u64,
    /// Last `origin_seq`, inclusive; `>= first`.
    pub last: u64,
}

/// One protocol message. See the module doc before touching the variant order.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Message {
    /// Both sides open with this. Where the HLC skew guard runs.
    Hello {
        /// The sender.
        device: DeviceId,
        /// The group it believes it shares with us.
        group: GroupId,
        /// What it already holds, per origin device.
        heads: Heads,
        /// Application protocol the sender speaks; `PROTOCOL_VERSION` for this build.
        protocol: u16,
    },
    /// The ops the sender is missing, derived by diffing heads. Empty means "in sync".
    Want {
        /// Runs to send.
        ranges: Vec<OriginRange>,
    },
    /// A batch of ops and the runs it covers.
    Ops {
        /// The ops, in a total order the receiver may apply as-is.
        ops: Vec<Op>,
        /// Which runs this batch completes.
        ranges: Vec<OriginRange>,
    },
    /// The runs the receiver has *committed* (not merely received), so a crash mid-import is
    /// re-requested rather than lost.
    Ack {
        /// Runs durably stored.
        committed: Vec<OriginRange>,
    },
}

/// Why a message could not be encoded or decoded.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MessageError {
    /// The envelope was wrong (bad magic, unknown version, too large, truncated).
    Frame(FrameError),
    /// A collection exceeds its cap; `what` names it.
    TooMany {
        /// Which collection.
        what: &'static str,
        /// Its length.
        len: usize,
        /// Its cap.
        max: usize,
    },
    /// A range runs backwards (`last < first`).
    BackwardsRange(OriginRange),
    /// postcard could not decode the body as this version's `Message`.
    Codec(postcard::Error),
    /// The body decoded but bytes were left over — a struct edit or a foreign message.
    TrailingBytes(usize),
}

impl fmt::Display for MessageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MessageError::Frame(e) => write!(f, "frame: {e}"),
            MessageError::TooMany { what, len, max } => {
                write!(f, "{what}: {len} entries exceed the cap of {max}")
            }
            MessageError::BackwardsRange(r) => {
                write!(
                    f,
                    "range {}..={} for {:?} runs backwards",
                    r.first, r.last, r.device
                )
            }
            MessageError::Codec(e) => write!(f, "postcard: {e}"),
            MessageError::TrailingBytes(n) => write!(f, "{n} bytes left after the message"),
        }
    }
}

impl std::error::Error for MessageError {}

impl From<FrameError> for MessageError {
    fn from(e: FrameError) -> MessageError {
        MessageError::Frame(e)
    }
}

impl Message {
    /// Encodes into a `Frame` for `PROTOCOL_VERSION`, after checking every cap.
    pub fn encode(&self) -> Result<Frame, MessageError> {
        self.check_caps()?;
        let body = postcard::to_allocvec(self).map_err(MessageError::Codec)?;
        debug_assert!(!body.is_empty(), "every variant encodes at least its tag");
        let frame = Frame::new(body)?;
        debug_assert_eq!(frame.version, PROTOCOL_VERSION);
        Ok(frame)
    }

    /// Decodes a `Frame` body, then checks every cap. A frame for another version is refused.
    pub fn decode(frame: &Frame) -> Result<Message, MessageError> {
        if frame.version != PROTOCOL_VERSION {
            return Err(FrameError::UnknownVersion {
                got: frame.version,
                supported: PROTOCOL_VERSION,
            }
            .into());
        }
        let (message, rest): (Message, &[u8]) =
            postcard::take_from_bytes(&frame.body).map_err(MessageError::Codec)?;
        if !rest.is_empty() {
            return Err(MessageError::TrailingBytes(rest.len()));
        }
        message.check_caps()?;
        debug_assert!(rest.is_empty());
        Ok(message)
    }

    /// Every collection under its cap and every range forwards. Exhaustive over the variants.
    pub fn check_caps(&self) -> Result<(), MessageError> {
        match self {
            Message::Hello { heads, .. } => cap("heads", heads.len(), MAX_HEADS),
            Message::Want { ranges } => ranges_ok("want ranges", ranges),
            Message::Ops { ops, ranges } => {
                cap("ops", ops.len(), MAX_OPS_PER_BATCH)?;
                ranges_ok("ops ranges", ranges)
            }
            Message::Ack { committed } => ranges_ok("ack ranges", committed),
        }
    }
}

fn cap(what: &'static str, len: usize, max: usize) -> Result<(), MessageError> {
    debug_assert!(max > 0, "a cap of zero would refuse every message");
    if len > max {
        return Err(MessageError::TooMany { what, len, max });
    }
    debug_assert!(len <= max);
    Ok(())
}

fn ranges_ok(what: &'static str, ranges: &[OriginRange]) -> Result<(), MessageError> {
    cap(what, ranges.len(), MAX_WANT_RANGES)?;
    for r in ranges {
        if r.last < r.first {
            return Err(MessageError::BackwardsRange(*r));
        }
    }
    debug_assert!(ranges.iter().all(|r| r.first <= r.last));
    Ok(())
}
