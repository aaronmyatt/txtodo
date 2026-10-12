//! A peer's ops minus the ones this device's log already holds, checked in the same turn that
//! lands them. `lan_apply::not_yet_stored` checks first, but outside that turn: a LAN and a relay
//! session with one peer can both pass it with the same run, and the second insert then failed the
//! `UNIQUE` op id and refused the batch (lab lan-to-relay, 2026-10-02). Each landing path (the
//! file actor, the notes actor, the log-only path) calls this where its commits are serialized.

use txtodo_model::Op;
use txtodo_store::{Store, StoreError};

/// `ops` without those whose id `store` holds, in order.
pub(crate) fn drop_held(store: &Store, ops: Vec<Op>) -> Result<Vec<Op>, StoreError> {
    let total = ops.len();
    let mut fresh = Vec::with_capacity(total);
    for op in ops {
        if store.op_by_id(op.id)?.is_none() {
            fresh.push(op);
        }
    }
    debug_assert!(fresh.len() <= total);
    Ok(fresh)
}
