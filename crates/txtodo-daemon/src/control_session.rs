//! Driving one control-channel session over one established `Link` connection (task
//! `daemon-workspace-identity-agreement` stage 5) — split out of `control_channel.rs` for its line
//! budget, the same pattern as `lan.rs`/`lan_session.rs`'s own split. Every session (whether
//! accepted or dialed) does the same symmetric thing: send this device's currently-registered
//! workspaces as `ControlMessage::Offer`s, then react to whatever the peer sends until the link
//! goes idle or errors (`IrohLink`'s own idle timeout, same "short session, redial periodically"
//! shape `lan_session.rs` already uses).

use std::sync::{Mutex, PoisonError};

use txtodo_model::{DeviceId, Ulid};
use txtodo_store::WorkspaceId;
use txtodo_sync::{ControlMessage, GroupId, GroupKey, GroupKeys, KeyId, Link};

use crate::device_identity::DeviceIdentity;
use crate::workspace_offer_registry::{PendingOffer, WorkspaceOfferRegistryError};
use crate::workspace_registry::WorkspaceRegistry;

/// Which group/epoch/key to seal under, bundled so `send_all_offers` stays under the
/// arg-count budget — the same reason `txtodo_sync::sealed_ops::SealContext` exists.
struct SealCtx<'a> {
    group: GroupId,
    epoch: u32,
    key: &'a GroupKey,
}

/// One full exchange over an established connection. Runs on the caller's own thread, which must
/// be a blocking one (`Link::send`/`recv` block, same invariant `lan_session.rs` documents).
pub(crate) fn drive_control_session(
    link: &mut dyn Link,
    identity: &DeviceIdentity,
    registry: &Mutex<WorkspaceRegistry>,
) {
    let key = fetch_group_key(identity);
    record_offers_outcome(identity, &key);
    let Some(key) = key.ok().flatten() else {
        return;
    };
    let epoch = identity.group_epoch();
    let Some(keys) = single_epoch_keys(epoch, key.clone()) else {
        return;
    };
    let ctx = SealCtx {
        group: identity.group(),
        epoch,
        key: &key,
    };

    if !send_all_offers(link, identity, registry, &ctx) {
        return;
    }
    while let Some(msg) = recv_control(link, ctx.group, &keys) {
        handle_one_message(identity, msg);
    }
}

/// The three ways this can fail (keystore error, no key stored yet, corrupt length) used to
/// collapse into one flat `control_channel_session_skipped_no_group_key` debug event at the call
/// site — each now logs its own specific reason here instead, at the source. `Err` carries what a
/// human should hear about (task control-channel-keystore-visibility); `Ok(None)` is "not paired
/// yet", which is no problem.
fn fetch_group_key(identity: &DeviceIdentity) -> Result<Option<GroupKey>, String> {
    let stored = match identity
        .key_store()
        .get(KeyId::Group(identity.group_epoch()))
    {
        Ok(v) => v,
        Err(e) => return Err(log_group_key_keystore_err(&e)),
    };
    let Some(bytes) = stored else {
        return Ok(log_group_key_missing());
    };
    let raw = bytes.expose();
    match raw.try_into() {
        Ok(array) => Ok(Some(GroupKey::from_bytes(array))),
        Err(_) => Err(log_group_key_corrupt(raw.len())),
    }
}

/// Puts the group-key read's outcome on the device's `LanStatus`, where `Health` and
/// `WorkspacePendingOffers` read it: a failure is kept with its time, a success clears it.
fn record_offers_outcome(identity: &DeviceIdentity, key: &Result<Option<GroupKey>, String>) {
    let problem = key.as_ref().err().map(|why| (why.clone(), now_ms()));
    identity.lan_status().set_offers_problem(problem);
}

fn log_group_key_keystore_err(e: &txtodo_sync::KeyStoreError) -> String {
    tracing::warn!(error = %e, "control_channel_group_key_keystore_error");
    format!("the keystore could not read the group key: {e}")
}

fn log_group_key_missing() -> Option<GroupKey> {
    tracing::debug!("control_channel_group_key_missing");
    None
}

fn log_group_key_corrupt(len: usize) -> String {
    tracing::warn!(len, "control_channel_group_key_corrupt_length");
    format!("the stored group key is {len} bytes, not 32")
}

fn single_epoch_keys(epoch: u32, key: GroupKey) -> Option<GroupKeys> {
    let mut keys = GroupKeys::new();
    match keys.insert(epoch, key) {
        Ok(()) => Some(keys),
        Err(e) => log_group_keys_insert_failed(&e),
    }
}

fn log_group_keys_insert_failed(e: &txtodo_sync::CryptoError) -> Option<GroupKeys> {
    tracing::warn!(error = %e, "control_channel_group_keys_insert_failed");
    None
}

/// This device's currently-active workspaces, as `(id, display name)` pairs — the name is derived
/// from the root directory's basename purely for an accept-side prompt (stage 6), never persisted
/// or used as identity.
pub(crate) fn outbound_offers(registry: &Mutex<WorkspaceRegistry>) -> Vec<(WorkspaceId, String)> {
    let registry = registry.lock().unwrap_or_else(PoisonError::into_inner);
    registry
        .list()
        .unwrap_or_default()
        .into_iter()
        .map(|entry| {
            let name = entry
                .root
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            (entry.id, name)
        })
        .collect()
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn send_control(link: &mut dyn Link, ctx: &SealCtx<'_>, msg: ControlMessage) -> Option<()> {
    let sealed = match txtodo_sync::seal_control(&msg, ctx.group, ctx.epoch, ctx.key) {
        Ok(frame) => frame,
        Err(e) => {
            log_seal_failed(&e);
            return Some(());
        }
    };
    link.send(sealed).ok()
}

fn log_seal_failed(e: &txtodo_sync::ControlSealError) {
    tracing::debug!(error = %e, "control_channel_seal_failed");
}

fn recv_control(link: &mut dyn Link, group: GroupId, keys: &GroupKeys) -> Option<ControlMessage> {
    let frame = link.recv().ok()?;
    match txtodo_sync::open_control(&frame, group, keys) {
        Ok(msg) => Some(msg),
        Err(e) => {
            log_open_failed(&e);
            None
        }
    }
}

fn log_open_failed(e: &txtodo_sync::ControlSealError) {
    tracing::debug!(error = %e, "control_channel_open_failed");
}

/// Sends this device's every currently-active workspace as a fresh `Offer`. Returns `false` the
/// moment the link fails, so the caller stops instead of trying the rest.
fn send_all_offers(
    link: &mut dyn Link,
    identity: &DeviceIdentity,
    registry: &Mutex<WorkspaceRegistry>,
    ctx: &SealCtx<'_>,
) -> bool {
    let device = identity.device();
    let now_ms = now_ms();
    for (workspace_id, name) in outbound_offers(registry) {
        // The default goes out under this device's alias (task default-workspace-pairing-consent):
        // a foreign peer mirrors it as a Remote workspace; an own device skips it, since it merges
        // that list under the reserved id already.
        let workspace_id = if workspace_id == crate::default_workspace::default_workspace_id() {
            crate::default_workspace::default_alias(device)
        } else {
            workspace_id
        };
        let msg = ControlMessage::Offer {
            sender: device,
            workspace_id: workspace_id.ulid().to_u128(),
            name,
            offered_at_ms: now_ms,
        };
        if send_control(link, ctx, msg).is_none() {
            return false;
        }
    }
    true
}

fn handle_one_message(identity: &DeviceIdentity, msg: ControlMessage) {
    match msg {
        ControlMessage::Offer {
            sender,
            workspace_id,
            name,
            offered_at_ms,
        } => record_offer(identity, sender, workspace_id, name, offered_at_ms),
        // Stage 6's own bookkeeping (e.g. stop re-offering a declined workspace to this peer) is
        // not built this pass — logged only, never silently dropped.
        ControlMessage::OfferAck { .. } | ControlMessage::Decline { .. } => log_offer_reply(&msg),
    }
}

fn record_offer(
    identity: &DeviceIdentity,
    sender: DeviceId,
    workspace_id: u128,
    name: String,
    offered_at_ms: u64,
) {
    let offer = PendingOffer {
        offering_device: sender,
        workspace_id: WorkspaceId::new(Ulid::from_u128(workspace_id)),
        name,
        offered_at_ms,
    };
    if let Err(e) = identity.workspace_offers().record(offer) {
        log_record_offer_failed(&e);
    }
}

fn log_record_offer_failed(e: &WorkspaceOfferRegistryError) {
    tracing::warn!(error = %e, "control_channel_record_offer_failed");
}

fn log_offer_reply(msg: &ControlMessage) {
    tracing::debug!(?msg, "control_channel_offer_reply_received");
}
