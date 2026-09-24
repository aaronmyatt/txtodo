//! Why the initiator answered a `JoinerHello` with `Rejected`, one logged reason each. Split out of
//! `pairing_lan.rs` for its line budget.

use txtodo_model::DeviceId;
use txtodo_sync::InitiatorReply;

/// No active session (expired window, wrong role, or none). `warn`: `Rejected` is fatal for the
/// joiner, and on 2026-09-23 an expired window (keychain prompts blocked the SAS confirm) left no
/// reason in the log at all.
pub(crate) fn reject_no_active_session(
    peer: DeviceId,
    e: &crate::pairing_state_error::PairingStateError,
) -> InitiatorReply {
    tracing::warn!(%peer, error = %e, "pairing_initiator_rejected_no_active_session");
    InitiatorReply::Rejected
}

/// Role/group/nonce mismatch — a stale retry, or a hello for a pairing this device never started.
pub(crate) fn reject_protocol_mismatch(peer: DeviceId) -> InitiatorReply {
    tracing::warn!(%peer, "pairing_initiator_rejected_protocol_mismatch");
    InitiatorReply::Rejected
}

/// A second device claimed a nonce/group already bound to someone else — a joiner race or a nonce
/// reuse attempt; `warn` so an operator can see it happened.
pub(crate) fn reject_peer_conflict(peer: DeviceId, bound_to: Option<DeviceId>) -> InitiatorReply {
    tracing::warn!(%peer, bound_to = ?bound_to, "pairing_initiator_rejected_peer_conflict");
    InitiatorReply::Rejected
}

/// The handshake's own crypto step refused this hello's public key — a real failure, not routine.
pub(crate) fn reject_handshake_failed(
    peer: DeviceId,
    e: &crate::pairing_state_error::PairingStateError,
) -> InitiatorReply {
    tracing::warn!(%peer, error = %e, "pairing_initiator_rejected_handshake_failed");
    InitiatorReply::Rejected
}
