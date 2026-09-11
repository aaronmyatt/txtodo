//! Workspace tree, task ids, HLC, op types. Plan M3; the CRDT (M4) reuses these shapes unchanged.
//!
//! Slice rule: depends on `txtodo-core` only. No I/O, no clock — callers pass `now_ms`.
#![forbid(unsafe_code)]

mod hlc;
mod ids;

pub use hlc::{Hlc, HlcError};
pub use ids::{DeviceId, FILE_PATH_MAX_BYTES, FilePath, FilePathError, OpId, TaskId, TokenId};
