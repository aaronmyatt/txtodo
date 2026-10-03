//! Peer ops stamped past this device's clock by more than the skew bound wait (lab clock-skew
//! 1072683562). The skew guard's decision is that a peer that far ahead does not get to send ops
//! (`tasks/model-hlc-skew-guard`, option A: its `Hello` is refused), but ops it made while ahead
//! still came once its clock was fixed. They were applied without their stamp merged into ours,
//! so our next edit sorted before lines it was typed after: a move that lost, a notes append
//! that no longer fit. Now `lan_apply::commit_incoming_ops` commits a batch only up to the first
//! such op and acks that prefix; the sender resends the rest every `RESEND_AFTER` until our clock
//! is within reach, and every op we apply is one our clock can merge.

use txtodo_model::Op;

use crate::lan_session::read;
use crate::server::SharedWorkspace;

/// Where `ops` reach a stamp more than `MAX_PEER_SKEW_AHEAD_MS` past this device's clock, the
/// bound `Hlc::merge` refuses.
pub(crate) fn first_ahead(ws: &SharedWorkspace, ops: &[Op]) -> Option<usize> {
    let now_ms = read(ws).clock().now_ms();
    let reach = now_ms.saturating_add(txtodo_model::MAX_PEER_SKEW_AHEAD_MS);
    ops.iter().position(|op| op.hlc.wall_ms > reach)
}

/// Logs an op held back and says why, for the batch's refusal (`stuck_sync.rs`).
pub(crate) fn held_back(op: &Op) -> String {
    tracing::info!(
        file = %op.file,
        op = %op.id.ulid(),
        wall_ms = op.hlc.wall_ms,
        "lan_sync_op_ahead_held_back"
    );
    format!(
        "an op stamped {} ms (wall) is past this clock's skew bound",
        op.hlc.wall_ms
    )
}
