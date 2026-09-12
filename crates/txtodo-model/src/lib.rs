//! Workspace tree, task ids, HLC, op types. Plan M3; the CRDT (M4) reuses these shapes unchanged.
//!
//! Slice rule: depends on `txtodo-core` only. No I/O, no clock — callers pass `now_ms`.
#![forbid(unsafe_code)]

mod hlc;
mod identity;
mod ids;
mod op;

pub use hlc::{Hlc, HlcError, MAX_PEER_SKEW_AHEAD_MS, MAX_PEER_SKEW_BEHIND_MS, Skew};
pub use identity::{CostWeights, Fingerprint, IdentityMode};
pub use ids::{DeviceId, FILE_PATH_MAX_BYTES, FilePath, FilePathError, OpId, TaskId, TokenId};
pub use op::{Field, FieldMismatch, FieldValue, Op, OpKind, Principal, TextEdit, set_field};
/// Re-exported so id-only consumers (store, daemon tests) need not depend on `txtodo-core`.
pub use txtodo_core::Ulid;

#[cfg(test)]
mod hlc_tests;
#[cfg(test)]
mod op_tests;
