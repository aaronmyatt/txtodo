//! Applying a batch of a peer's ops to a document, tolerant of order inside one commit. Moved
//! from the daemon's `sync_ops.rs` (ADR 0042): the daemon's sync path and the parking retry
//! (`sync_park`) both use it, and so will the web peer.

use crate::state::{DocState, StateError};
use txtodo_model::Op;

/// Applies `ops` to `state` one commit (same HLC stamp) at a time; within a commit, an op that
/// fails is retried after the others, until a round applies nothing new. Returns the ops that
/// never applied, with why.
pub fn apply_leniently<'a>(state: &mut DocState, ops: &'a [Op]) -> Vec<(&'a Op, StateError)> {
    let mut skipped = Vec::new();
    for group in ops.chunk_by(|a, b| a.hlc == b.hlc) {
        let mut pending: Vec<&Op> = group.iter().collect();
        // Bounded: every round but the last applies at least one op.
        loop {
            let before = pending.len();
            let mut failed = Vec::new();
            for op in pending {
                if let Err(e) = state.apply(op) {
                    failed.push((op, e));
                }
            }
            if failed.is_empty() || failed.len() == before {
                skipped.extend(failed);
                break;
            }
            pending = failed.into_iter().map(|(op, _)| op).collect();
        }
    }
    debug_assert!(skipped.len() <= ops.len());
    skipped
}
