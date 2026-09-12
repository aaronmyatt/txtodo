//! The outermost wire envelope. **Layout frozen forever**: `MAGIC` · `version: u16 LE` ·
//! `len: u32 LE` · `body[len]`. Everything that may ever change lives inside `body`, decoded by
//! the `Message` type for that `version`. postcard is not self-describing
//! (<https://postcard.jamesmunns.com/wire-format>): a struct that grows a field does not fail to
//! decode old bytes, it decodes garbage — so the version cannot live inside a postcard struct, and
//! this header is hand-laid rather than serialised.

use std::fmt;

/// First four bytes of every frame; a foreign or truncated stream fails here, not in a message.
pub const MAGIC: [u8; 4] = *b"TXTO";
/// The one protocol version this build speaks. Bump only with a new `Message` decoder beside it.
pub const PROTOCOL_VERSION: u16 = 1;
/// Largest body we will decode. Checked against the header *before* any allocation.
pub const MAX_FRAME_BYTES: usize = 4 * 1024 * 1024;
/// Header size in bytes: magic (4) + version (2) + len (4).
pub const HEADER_BYTES: usize = 10;

/// One frame: a version and an opaque body.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame {
    /// Which `Message` decoder understands `body`.
    pub version: u16,
    /// The postcard-encoded message for that version.
    pub body: Vec<u8>,
}

/// Why bytes are not a frame we accept. Nothing is consumed on any `Err`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FrameError {
    /// The first four bytes were not `MAGIC`.
    BadMagic {
        /// What was there instead (zero-padded if fewer than four bytes arrived).
        got: [u8; 4],
    },
    /// Fewer bytes than the header, or than the header's `len`, were available.
    Truncated {
        /// Bytes the frame needs in total.
        needed: usize,
        /// Bytes available.
        got: usize,
    },
    /// The header's `len` exceeds `MAX_FRAME_BYTES`. Rejected before allocating.
    TooLarge {
        /// The claimed body length.
        len: usize,
        /// The cap.
        max: usize,
    },
    /// The frame is well-formed but for a version this build does not speak.
    UnknownVersion {
        /// The version on the wire.
        got: u16,
        /// The one version we decode.
        supported: u16,
    },
}

impl fmt::Display for FrameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FrameError::BadMagic { got } => write!(f, "frame magic {got:?} is not {MAGIC:?}"),
            FrameError::Truncated { needed, got } => {
                write!(f, "frame needs {needed} bytes, only {got} available")
            }
            FrameError::TooLarge { len, max } => {
                write!(f, "frame body of {len} bytes exceeds the {max} byte cap")
            }
            FrameError::UnknownVersion { got, supported } => {
                write!(
                    f,
                    "frame version {got} is not the supported version {supported}"
                )
            }
        }
    }
}

impl std::error::Error for FrameError {}

impl Frame {
    /// Wraps a body for `PROTOCOL_VERSION`.
    pub fn new(body: Vec<u8>) -> Result<Frame, FrameError> {
        if body.len() > MAX_FRAME_BYTES {
            return Err(FrameError::TooLarge {
                len: body.len(),
                max: MAX_FRAME_BYTES,
            });
        }
        Ok(Frame {
            version: PROTOCOL_VERSION,
            body,
        })
    }

    /// The bytes on the wire: header then body.
    pub fn encode(&self) -> Result<Vec<u8>, FrameError> {
        if self.body.len() > MAX_FRAME_BYTES {
            return Err(FrameError::TooLarge {
                len: self.body.len(),
                max: MAX_FRAME_BYTES,
            });
        }
        let len = u32::try_from(self.body.len()).map_err(|_| FrameError::TooLarge {
            len: self.body.len(),
            max: MAX_FRAME_BYTES,
        })?;
        let mut out = Vec::with_capacity(HEADER_BYTES + self.body.len());
        out.extend_from_slice(&MAGIC);
        out.extend_from_slice(&self.version.to_le_bytes());
        out.extend_from_slice(&len.to_le_bytes());
        out.extend_from_slice(&self.body);
        debug_assert_eq!(out.len(), HEADER_BYTES + self.body.len());
        debug_assert_eq!(&out[..4], &MAGIC);
        Ok(out)
    }

    /// Parses the header only: `(version, total frame bytes)`. Lets a caller skip a frame it
    /// cannot decode. Rejects bad magic and an over-cap length without touching the body.
    pub fn peek(bytes: &[u8]) -> Result<(u16, usize), FrameError> {
        if bytes.len() < HEADER_BYTES {
            if bytes.len() >= MAGIC.len() && bytes[..4] != MAGIC {
                return Err(bad_magic(bytes));
            }
            return Err(FrameError::Truncated {
                needed: HEADER_BYTES,
                got: bytes.len(),
            });
        }
        if bytes[..4] != MAGIC {
            return Err(bad_magic(bytes));
        }
        let version = u16::from_le_bytes([bytes[4], bytes[5]]);
        let len = u32::from_le_bytes([bytes[6], bytes[7], bytes[8], bytes[9]]) as usize;
        if len > MAX_FRAME_BYTES {
            return Err(FrameError::TooLarge {
                len,
                max: MAX_FRAME_BYTES,
            });
        }
        debug_assert!(len <= MAX_FRAME_BYTES);
        debug_assert!(
            HEADER_BYTES + len >= HEADER_BYTES,
            "no overflow under the cap"
        );
        Ok((version, HEADER_BYTES + len))
    }

    /// Decodes one frame from the front of `bytes`: `(frame, bytes consumed)`. On `Err` nothing
    /// is consumed and nothing was allocated.
    pub fn decode(bytes: &[u8]) -> Result<(Frame, usize), FrameError> {
        let (version, total) = Frame::peek(bytes)?;
        if version != PROTOCOL_VERSION {
            return Err(FrameError::UnknownVersion {
                got: version,
                supported: PROTOCOL_VERSION,
            });
        }
        if bytes.len() < total {
            return Err(FrameError::Truncated {
                needed: total,
                got: bytes.len(),
            });
        }
        let body = bytes[HEADER_BYTES..total].to_vec();
        debug_assert_eq!(body.len() + HEADER_BYTES, total);
        debug_assert!(body.len() <= MAX_FRAME_BYTES);
        Ok((Frame { version, body }, total))
    }
}

fn bad_magic(bytes: &[u8]) -> FrameError {
    let mut got = [0u8; 4];
    let n = bytes.len().min(4);
    got[..n].copy_from_slice(&bytes[..n]);
    debug_assert!(got != MAGIC, "bad_magic is only built for a mismatch");
    debug_assert!(n <= 4);
    FrameError::BadMagic { got }
}
