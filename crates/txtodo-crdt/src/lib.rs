//! Loro document, ops to/from Loro, reconciler.
#![forbid(unsafe_code)]

mod doc;
mod lww;

pub use doc::{
    BLANK_TAG, DESCRIPTION_KEY, FILES_PREFIX, LoroDocument, TASKS_MAP, blank_id, is_blank,
};
pub use lww::{HLC_BYTES, Lww, decode, decode_hlc, encode_hlc, read, write_if_newer};
