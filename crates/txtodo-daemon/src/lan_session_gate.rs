//! Which of this device's open workspaces one sync session carries (task
//! `default-workspace-pairing-consent`, decided 2026-09-24, option A). Every workspace, except that
//! the default merges under ADR 0029's reserved id only with a peer both humans called their own
//! device (`devices.own_device`). With any other peer the reserved id is left out, and this
//! device's default goes under its alias instead (`default_workspace::default_alias`), which that
//! peer mirrors as a separate Remote workspace: the two defaults never merge.
//!
//! An unknown peer (one this device never paired with directly, say a device that joined the
//! group through another) counts as not own: the stricter reading, so it sees this device's
//! default as a Remote entry rather than merging with it.

use std::collections::BTreeMap;
use std::sync::PoisonError;

use txtodo_model::DeviceId;
use txtodo_store::WorkspaceId;

use crate::default_workspace::{default_alias, default_workspace_id};
use crate::device_relay::WorkspaceRoute;
use crate::lan_session::read;

/// `all` for an own-device `peer`; otherwise `all` with the reserved default moved to `device`'s
/// alias.
pub(crate) fn session_routes(
    all: &BTreeMap<WorkspaceId, WorkspaceRoute>,
    device: DeviceId,
    peer: DeviceId,
) -> BTreeMap<WorkspaceId, WorkspaceRoute> {
    let mut routes = all.clone();
    if is_own_peer(all, peer) {
        return routes;
    }
    if let Some(default) = routes.remove(&default_workspace_id()) {
        routes.insert(default_alias(device), default);
        tracing::info!(%peer, "lan_session_default_kept_apart_for_a_foreign_peer");
    }
    routes
}

/// Whether the device-global `devices` table records `peer` as own. Any route's workspace reaches
/// that table (ADR 0021); with none open there is nothing to carry anyway.
fn is_own_peer(all: &BTreeMap<WorkspaceId, WorkspaceRoute>, peer: DeviceId) -> bool {
    let Some(route) = all.values().next() else {
        return false;
    };
    read(&route.ws)
        .identity_store()
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .is_own_device(peer)
        .unwrap_or(false)
}
