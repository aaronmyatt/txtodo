//! Own carries across one shared device (ADR 0029, 2026-10-02 amendment). In each control session
//! a device sends its direct own devices to an own peer (`ControlMessage::OwnDevices`), once, on
//! the first message that names a direct own sender; and takes such a list only from a direct own
//! peer (`IdentityStore::set_own_vouches`). Control sessions run every resync tick, so a pairing
//! or a removal reaches every own peer within one. When the own set changes, live sync sessions end
//! and greet again (`LivePeers::own_generation`), so the default moves off an alias at once.

use std::sync::PoisonError;

use txtodo_model::DeviceId;
use txtodo_sync::{ControlMessage, MAX_OWN_DEVICES};

use crate::device_identity::DeviceIdentity;

/// The `OwnDevices` this device owes `peer`: `None` unless `peer` is a direct own device. The
/// list leaves `peer` out and holds direct own devices only, never vouched ones (one hop).
pub(crate) fn own_devices_for(identity: &DeviceIdentity, peer: DeviceId) -> Option<ControlMessage> {
    let store = identity
        .store()
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    if !store.is_direct_own(peer).unwrap_or(false) {
        return None;
    }
    let devices: Vec<DeviceId> = store
        .direct_own_devices()
        .unwrap_or_default()
        .into_iter()
        .filter(|d| *d != peer)
        .take(MAX_OWN_DEVICES)
        .collect();
    Some(ControlMessage::OwnDevices {
        sender: identity.device(),
        devices,
    })
}

/// Takes `sender`'s list when `sender` is a direct own device; anything else is logged and
/// dropped. This device is left out of what is stored. A change to the own set ends live sync
/// sessions so they greet again under it.
pub(crate) fn record_own_devices(
    identity: &DeviceIdentity,
    sender: DeviceId,
    devices: &[DeviceId],
) {
    let me = identity.device();
    let kept: Vec<DeviceId> = devices.iter().copied().filter(|d| *d != me).collect();
    if store_vouches(identity, sender, &kept) == Some(true) {
        log_own_changed(sender);
        identity.live_peers().bump_own_generation();
    }
}

/// `Some(changed)` once `kept` is stored as `sender`'s list; `None` (logged) when `sender` is not
/// a direct own device or the store failed.
fn store_vouches(identity: &DeviceIdentity, sender: DeviceId, kept: &[DeviceId]) -> Option<bool> {
    let mut store = identity
        .store()
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    if !store.is_direct_own(sender).unwrap_or(false) {
        return log_ignored(sender);
    }
    let before = store.vouched_own_devices().unwrap_or_default();
    store
        .set_own_vouches(sender, kept)
        .map_err(|e| log_store_failed(sender, &e))
        .ok()?;
    Some(store.vouched_own_devices().unwrap_or_default() != before)
}

/// tracing: <https://docs.rs/tracing/latest/tracing/macro.info.html>
fn log_own_changed(sender: DeviceId) {
    tracing::info!(%sender, "own_devices_changed_sessions_greet_again");
}

fn log_ignored(sender: DeviceId) -> Option<bool> {
    tracing::debug!(%sender, "own_devices_ignored_not_direct_own");
    None
}

fn log_store_failed(sender: DeviceId, e: &txtodo_store::StoreError) {
    tracing::warn!(%sender, error = %e, "own_devices_store_failed");
}

/// The sender every control message names.
pub(crate) fn sender_of(msg: &ControlMessage) -> DeviceId {
    match msg {
        ControlMessage::Offer { sender, .. }
        | ControlMessage::OfferAck { sender, .. }
        | ControlMessage::Decline { sender, .. }
        | ControlMessage::OwnDevices { sender, .. } => *sender,
    }
}
