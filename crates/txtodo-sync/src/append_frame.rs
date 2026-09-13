//! Length-prefixed framing for one `.ops` file on disk (plan M8 `sync-file-carrier`, design §4.5).
//! This is deliberately a *different* concept from [`crate::frame::Frame`] (the frozen wire
//! envelope moved over a [`crate::Link`]): a file that dumb file sync (Syncthing / Dropbox / iCloud
//! Drive) is mid-copying is not a message channel, it is a byte stream that can end at any point
//! between two writes. `AppendFrame` exists so a scanner can tell "not written yet" apart from
//! "corrupt" without touching `Frame`'s own frozen layout.
//!
//! `body` is opaque here: `carrier.rs` puts an encoded [`crate::Frame`] (already ciphertext, once
//! `sync-crypto-envelope` seals it) inside, but this module never looks inside `body` itself.

use crate::carrier_error::CarrierError;

/// Largest single append-frame body this build will decode from a `.ops` file. Bounds an
/// allocation attempt against a corrupt length the same way [`crate::frame::MAX_FRAME_BYTES`]
/// bounds `Frame` itself; a `Frame` never exceeds that cap, so this one is generous around it.
pub const MAX_APPEND_FRAME_BYTES: usize = 8 * 1024 * 1024;
/// Header size in bytes: `len: u32 LE`.
pub const APPEND_HEADER_BYTES: usize = 4;

/// One length-prefixed record in a `.ops` file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppendFrame {
    /// `body`'s length, redundant with `body.len()` but kept as its own field the same way
    /// `Frame`'s header carries `len` rather than relying on the caller to have measured `body`.
    pub len: u32,
    /// The opaque payload — an encoded [`crate::Frame`], from this module's point of view just bytes.
    pub body: Vec<u8>,
}

impl AppendFrame {
    /// Wraps `body`. `Err` only if it exceeds [`MAX_APPEND_FRAME_BYTES`]; every other byte pattern
    /// is representable.
    pub fn new(body: Vec<u8>) -> Result<AppendFrame, CarrierError> {
        if body.len() > MAX_APPEND_FRAME_BYTES {
            return Err(CarrierError::FrameTooLarge {
                len: body.len(),
                max: MAX_APPEND_FRAME_BYTES,
            });
        }
        let len = u32::try_from(body.len()).map_err(|_| CarrierError::FrameTooLarge {
            len: body.len(),
            max: MAX_APPEND_FRAME_BYTES,
        })?;
        Ok(AppendFrame { len, body })
    }

    /// The bytes to append to the file: `len` then `body`. Appending this to a file that already
    /// ends on a frame boundary keeps every prior record byte-for-byte untouched — this only ever
    /// grows a file, never rewrites it.
    pub fn encode(&self) -> Vec<u8> {
        debug_assert_eq!(self.len as usize, self.body.len(), "len tracks body");
        let mut out = Vec::with_capacity(APPEND_HEADER_BYTES + self.body.len());
        out.extend_from_slice(&self.len.to_le_bytes());
        out.extend_from_slice(&self.body);
        out
    }

    /// Decodes one frame from the front of `bytes`: `Some((frame, bytes consumed))`.
    ///
    /// `None` — **not an error** — when `bytes` holds an incomplete trailing frame: a header with
    /// no full body yet, or fewer than [`APPEND_HEADER_BYTES`] bytes at all. That is the normal
    /// shape of a file mid-copy by file-sync software, or our own most recent write still landing;
    /// the caller re-reads later and gets the completed frame once more bytes exist on disk.
    ///
    /// `Err` only for a length that can never be satisfied because it exceeds
    /// [`MAX_APPEND_FRAME_BYTES`] — that is corruption (or a foreign file), not a partial write, so
    /// it is never confused with the `None` case above.
    pub fn try_decode(bytes: &[u8]) -> Result<Option<(AppendFrame, usize)>, CarrierError> {
        if bytes.len() < APPEND_HEADER_BYTES {
            return Ok(None);
        }
        let len = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        let len_usize = len as usize;
        if len_usize > MAX_APPEND_FRAME_BYTES {
            return Err(CarrierError::FrameTooLarge {
                len: len_usize,
                max: MAX_APPEND_FRAME_BYTES,
            });
        }
        let total = APPEND_HEADER_BYTES + len_usize;
        debug_assert!(total >= APPEND_HEADER_BYTES, "no overflow under the cap");
        if bytes.len() < total {
            return Ok(None);
        }
        let body = bytes[APPEND_HEADER_BYTES..total].to_vec();
        Ok(Some((AppendFrame { len, body }, total)))
    }
}
