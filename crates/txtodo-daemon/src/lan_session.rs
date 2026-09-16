//! Crypto-material lookups shared by every real `Link`-driving path in this crate (plan M4
//! `sync-lan-transport`; task `daemon-workspace-session-multiplex` stage 2 split this file down to
//! just these): fetching a workspace's group key, building its single-epoch `GroupKeys`, and
//! reading its current heads. `lan_session_shared.rs` is where the actual multiplexed read/write
//! loop now lives (`drive_shared_session`) — this file's own [`drive_session`] is the
//! single-workspace backward-compat wrapper every pre-stage-2 call site (`lan.rs::spawn_driver`,
//! `relay.rs`, every existing single-workspace test) still calls, built by handing
//! `drive_shared_session` a routing table with exactly one entry: unchanged behaviour, same code
//! path as a genuinely multi-workspace connection with only one workspace open.

use std::sync::PoisonError;

use txtodo_model::DeviceId;
use txtodo_sync::{GroupId, GroupKey, GroupKeys, KeyId, Link};

use crate::device_relay::{WorkspaceRoute, WorkspaceRoutes};
use crate::server::SharedWorkspace;
use crate::workspace::Workspace;

/// Group-key epoch this pass always uses. Rotation (`sync-device-remove`) will need to make the
/// epoch a live value read off the session instead of a constant.
pub(crate) const GROUP_EPOCH: u32 = 0;

pub(crate) fn read(ws: &SharedWorkspace) -> std::sync::RwLockReadGuard<'_, Workspace> {
    ws.read().unwrap_or_else(PoisonError::into_inner)
}

pub(crate) fn write(ws: &SharedWorkspace) -> std::sync::RwLockWriteGuard<'_, Workspace> {
    ws.write().unwrap_or_else(PoisonError::into_inner)
}

/// `pub(crate)`: `file_carrier.rs` (plan M8 `relay-converge-test`) reuses this too — the file
/// carrier's send/receive path needs the same group key `drive_shared_session` does, with no
/// `Session` of its own (see that module's doc for why it does not reuse the driver wholesale).
pub(crate) fn fetch_group_key(ws: &SharedWorkspace) -> Option<GroupKey> {
    let bytes = read(ws).key_store().get(KeyId::Group(GROUP_EPOCH)).ok()??;
    let array: [u8; txtodo_sync::KEY_BYTES] = bytes.expose().try_into().ok()?;
    Some(GroupKey::from_bytes(array))
}

/// `pub(crate)`: see [`fetch_group_key`]'s doc.
pub(crate) fn single_epoch_keys(key: GroupKey) -> Option<GroupKeys> {
    let mut keys = GroupKeys::new();
    keys.insert(GROUP_EPOCH, key).ok()?;
    Some(keys)
}

/// `pub(crate)`: see [`fetch_group_key`]'s doc.
pub(crate) fn read_heads(ws: &SharedWorkspace) -> txtodo_sync::Heads {
    let store = read(ws).store().clone();
    store
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .heads()
        .unwrap_or_default()
}

/// The single-workspace entry point every pre-stage-2 call site still uses: builds a one-entry
/// routing table for `ws` and hands it to `lan_session_shared::drive_shared_session`, so a peer
/// relationship with only one open workspace behaves exactly as it did before this stage (same
/// code path, not a parallel one that could drift).
pub(crate) fn drive_session(
    link: &mut dyn Link,
    ws: SharedWorkspace,
    device: DeviceId,
    group: GroupId,
) {
    let id = read(&ws).workspace_id();
    let routes = WorkspaceRoutes::new();
    let registered = routes.register(id, WorkspaceRoute { ws, device, group });
    debug_assert!(
        registered.is_ok(),
        "a single-entry routing table never exceeds MAX_ROUTED_WORKSPACES"
    );
    crate::lan_session_dispatch::drive_shared_session(link, &routes, device, group);
}
