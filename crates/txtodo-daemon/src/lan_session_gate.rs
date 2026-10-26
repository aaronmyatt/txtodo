//! Which of this device's open workspaces one sync session carries (task
//! `default-workspace-pairing-consent`, decided 2026-09-24, option A; task
//! `two-kinds-of-workspace`, ADR 0043). The default and every named list are private to the user's
//! own devices (`devices.own_device`): an own peer gets them all, and any other peer gets neither a
//! route for them nor an alias. The default used to reach a foreign peer under an alias, as a
//! separate Remote workspace the two never merged; that went with the Remote mirror, since a list is
//! private and sharing with another person is what git is for. A repo is on no route table at all.
//!
//! An unknown peer (one this device never paired with directly, say a device that joined the
//! group through another) counts as not own: the stricter reading.

use std::collections::BTreeMap;
use std::sync::PoisonError;

use txtodo_model::DeviceId;
use txtodo_store::WorkspaceId;

use crate::default_workspace::default_workspace_id;
use crate::device_relay::WorkspaceRoute;
use crate::lan_session::read;
use crate::list_workspace::is_list_id;

/// `all` for an own-device `peer`; otherwise `all` without the default and the named lists.
pub(crate) fn session_routes(
    all: &BTreeMap<WorkspaceId, WorkspaceRoute>,
    peer: DeviceId,
) -> BTreeMap<WorkspaceId, WorkspaceRoute> {
    let mut routes = all.clone();
    if is_own_peer(all, peer) {
        return routes;
    }
    drop_private(&mut routes, peer);
    routes
}

/// Takes the default and every named list out of `routes`: both are private to the user's own
/// devices.
fn drop_private(routes: &mut BTreeMap<WorkspaceId, WorkspaceRoute>, peer: DeviceId) {
    let before = routes.len();
    routes.retain(|id, _| *id != default_workspace_id() && !is_list_id(*id));
    if routes.len() < before {
        let kept_back = before - routes.len();
        tracing::info!(%peer, kept_back, "lan_session_private_workspaces_kept_from_a_foreign_peer");
    }
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
