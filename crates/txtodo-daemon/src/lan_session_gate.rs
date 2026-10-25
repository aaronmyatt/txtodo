//! Which of this device's open workspaces one sync session carries (task
//! `default-workspace-pairing-consent`, decided 2026-09-24, option A; task
//! `two-kinds-of-workspace`, ADR 0043). Every workspace goes to every peer, except two kinds that
//! are private to the user's own devices (`devices.own_device`):
//!
//! - the default merges under ADR 0029's reserved id only with a peer both humans called their own
//!   device. With any other peer the reserved id is left out, and this device's default goes under
//!   its alias instead (`default_workspace::default_alias`), which that peer mirrors as a separate
//!   Remote workspace: the two defaults never merge;
//! - a named list (`list_workspace::is_list_id`) goes to an own peer and to nobody else. A foreign
//!   peer gets no route for it and no alias: a list is private, and sharing with another person is
//!   what git is for.
//!
//! An unknown peer (one this device never paired with directly, say a device that joined the
//! group through another) counts as not own: the stricter reading.

use std::collections::BTreeMap;
use std::sync::PoisonError;

use txtodo_model::DeviceId;
use txtodo_store::WorkspaceId;

use crate::default_workspace::{default_alias, default_workspace_id};
use crate::device_relay::WorkspaceRoute;
use crate::lan_session::read;
use crate::list_workspace::is_list_id;

/// `all` for an own-device `peer`; otherwise `all` without the named lists and with the reserved
/// default moved to `device`'s alias.
pub(crate) fn session_routes(
    all: &BTreeMap<WorkspaceId, WorkspaceRoute>,
    device: DeviceId,
    peer: DeviceId,
) -> BTreeMap<WorkspaceId, WorkspaceRoute> {
    let mut routes = all.clone();
    if is_own_peer(all, peer) {
        return routes;
    }
    drop_lists(&mut routes, peer);
    if let Some(default) = routes.remove(&default_workspace_id()) {
        routes.insert(default_alias(device), default);
        tracing::info!(%peer, "lan_session_default_kept_apart_for_a_foreign_peer");
    }
    routes
}

/// Takes every named list out of `routes`: a list is private to the user's own devices.
fn drop_lists(routes: &mut BTreeMap<WorkspaceId, WorkspaceRoute>, peer: DeviceId) {
    let before = routes.len();
    routes.retain(|id, _| !is_list_id(*id));
    if routes.len() < before {
        let lists = before - routes.len();
        tracing::info!(%peer, lists, "lan_session_lists_kept_from_a_foreign_peer");
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
