//! The one-line log events of `lan_session_dispatch.rs`, each in its own tiny function: a
//! `tracing` macro counts heavily toward a function's cognitive complexity (`clippy.toml`), so a
//! caller with several branches logs through these instead of inline.

use txtodo_model::DeviceId;
use txtodo_store::WorkspaceId;

pub(crate) fn log_frame_too_short_to_peek() {
    tracing::debug!("lan_session_frame_too_short_to_peek");
}

/// `false`, so the caller can return it as "do not greet".
pub(crate) fn log_removed_peer_refused(peer: DeviceId) -> bool {
    tracing::info!(%peer, "lan_session_removed_peer_refused");
    false
}

pub(crate) fn log_unrouted_workspace_skipped(workspace: WorkspaceId) {
    tracing::debug!(%workspace, "lan_session_unrouted_workspace_message_skipped");
}

/// The set of own devices changed since this session greeted: end it, and the reconnect greets
/// again (task `sync-live-push`).
pub(crate) fn log_own_devices_changed() -> Option<()> {
    tracing::info!("lan_session_own_devices_changed");
    None
}

/// A workspace opened or closed since this session greeted its set: end it, and the reconnect
/// greets the new set (task `sync-live-push`).
pub(crate) fn log_routes_changed() -> Option<()> {
    tracing::debug!("lan_session_routes_changed_reconnecting");
    None
}

pub(crate) fn log_no_routed_workspace() {
    tracing::debug!("lan_session_skipped_no_routed_workspace");
}

pub(crate) fn log_no_group_key() {
    tracing::debug!("lan_session_skipped_no_group_key");
}
