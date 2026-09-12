//! Loro document, ops to/from Loro, reconciler.
#![forbid(unsafe_code)]

mod lww;

pub use lww::{HLC_BYTES, Lww, decode, decode_hlc, encode_hlc, read, write_if_newer};
