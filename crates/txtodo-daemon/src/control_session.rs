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
use txtodo_sync::{ControlMessage, ControlSealError, GroupId, GroupKey, GroupKeys, Link};

use crate::control_flow::Control;
use crate::device_identity::DeviceIdentity;
use crate::peer_keys::PeerSignal;
use crate::workspace_offer_registry::{
    PendingOffer, WorkspaceOfferRegistry, WorkspaceOfferRegistryError,
};
use crate::workspace_registry::WorkspaceRegistry;

/// Which group/epoch/key to seal under, bundled so `send_all_offers` stays under the
/// arg-count budget — the same reason `txtodo_sync::sealed_ops::SealContext` exists.
pub(crate) struct SealCtx<'a> {
    pub(crate) group: GroupId,
    pub(crate) epoch: u32,
    pub(crate) key: &'a GroupKey,
}

/// One full exchange over an established connection. Runs on the caller's own thread, which must
/// be a blocking one (`Link::send`/`recv` block, same invariant `lan_session.rs` documents).
/// `dialed` is the peer this session dialed, `None` when it accepted: the dialer speaks first
/// (`control_flow.rs`). Returns what the peer's frames showed about its key, for `peer_keys.rs`
/// (task sync-drift line 5).
pub(crate) fn drive_control_session(
    link: &mut dyn Link,
    identity: &DeviceIdentity,
    registry: &Mutex<WorkspaceRegistry>,
    dialed: Option<DeviceId>,
) -> PeerSignal {
    let key = fetch_group_key(identity);
    record_offers_outcome(identity, &key);
    if key.ok().flatten().is_none() {
        return PeerSignal::Silent;
    }
    let epoch = identity.group_epoch();
    let Some(keys) = crate::group_epoch::retained(identity.key_store().as_ref(), epoch) else {
        return PeerSignal::Silent;
    };
    let mut control = Control::new(link, identity, registry, keys, dialed);
    if !control.start() {
        return PeerSignal::Silent;
    }
    control.run()
}

/// The current epoch's group key. A keystore error, no key stored yet and a corrupt length used to
/// collapse into one flat `control_channel_session_skipped_no_group_key` debug event at the call
/// site — each now logs its own specific reason here instead, at the source. `Err` carries what a
/// human should hear about (task control-channel-keystore-visibility); `Ok(None)` is "not paired
/// yet", which is no problem.
fn fetch_group_key(identity: &DeviceIdentity) -> Result<Option<GroupKey>, String> {
    let fetched = crate::group_epoch::fetch(identity.key_store().as_ref(), identity.group_epoch());
    log_group_key_read(&fetched);
    fetched
}

fn log_group_key_read(fetched: &Result<Option<GroupKey>, String>) {
    match fetched {
        Err(why) => log_group_key_unreadable(why),
        Ok(None) => log_group_key_missing(),
        Ok(Some(_)) => {}
    }
}

fn log_group_key_unreadable(why: &str) {
    tracing::warn!(why, "control_channel_group_key_unreadable");
}

fn log_group_key_missing() {
    tracing::debug!("control_channel_group_key_missing");
}

/// Puts the group-key read's outcome on the device's `LanStatus`, where `Health` and
/// `WorkspacePendingOffers` read it: a failure is kept with its time, a success clears it.
fn record_offers_outcome(identity: &DeviceIdentity, key: &Result<Option<GroupKey>, String>) {
    let problem = key.as_ref().err().map(|why| (why.clone(), now_ms()));
    identity.lan_status().set_offers_problem(problem);
}

/// This device's lists, as `(id, display name)` pairs — never used as identity. Only lists
/// (`is_list_id`: a list's id comes from its name) are offered (ADR 0043). A repo is carried by
/// git, so no peer is told it exists; the default is on every device already, under one id, and a
/// foreign peer is shown nothing. The name is the one this device shows (`workspace_name::display_name`, task
/// workspace-vanity-name): the `txtodo.toml` name, else the folder's, else for a mirror what its
/// own offering device called it (`offers`), so a third device learns the name, not the id.
pub(crate) fn outbound_offers(
    registry: &Mutex<WorkspaceRegistry>,
    offers: &WorkspaceOfferRegistry,
) -> Vec<(WorkspaceId, String)> {
    let entries = registry
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .list()
        .unwrap_or_default();
    entries
        .into_iter()
        .filter(|entry| crate::list_workspace::is_list_id(entry.id))
        .map(|entry| {
            let name = crate::workspace_name::display_name(entry.id, &entry.root, false, || {
                offers.offered_name(entry.id)
            });
            (entry.id, name)
        })
        .collect()
}

pub(crate) fn now_ms() -> u64 {
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

/// `Err(Silent)` once the link is closed or idle, `Err(OpenFailed(kind))` for a frame that did not
/// open (logged here at debug; the once-per-peer warning is `peer_keys.rs`'s).
pub(crate) fn recv_control(
    link: &mut dyn Link,
    group: GroupId,
    keys: &GroupKeys,
) -> Result<ControlMessage, PeerSignal> {
    let frame = link.recv().map_err(|e| match e {
        // A peer on another sync protocol (task sync-divergence-check/protocol-mismatch).
        txtodo_sync::LinkError::Frame(txtodo_sync::FrameError::UnknownVersion { got, .. }) => {
            PeerSignal::OtherProtocol(got)
        }
        _ => PeerSignal::Silent,
    })?;
    txtodo_sync::open_control(&frame, group, keys).map_err(|e| {
        let kind = control_error_kind(&e);
        tracing::debug!(error = %e, kind, "control_channel_open_failed");
        PeerSignal::OpenFailed(kind)
    })
}

/// Same tags as a sync frame's (`lan_session_shared::crypto_error_kind`), so `wrong_group` means
/// one thing whichever channel saw it.
fn control_error_kind(e: &ControlSealError) -> &'static str {
    match e {
        ControlSealError::Crypto(c) => crate::lan_session_shared::crypto_error_kind(c),
        ControlSealError::Message(_) => "control_message",
    }
}

/// Sends this device's every currently-active workspace as a fresh `Offer`. Returns `false` the
/// moment the link fails, so the caller stops instead of trying the rest.
pub(crate) fn send_all_offers(
    link: &mut dyn Link,
    identity: &DeviceIdentity,
    registry: &Mutex<WorkspaceRegistry>,
    ctx: &SealCtx<'_>,
) -> bool {
    let device = identity.device();
    let now_ms = now_ms();
    for (workspace_id, name) in outbound_offers(registry, identity.workspace_offers()) {
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

pub(crate) fn handle_one_message(identity: &DeviceIdentity, msg: ControlMessage) {
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
        ControlMessage::OwnDevices { sender, devices } => {
            crate::own_vouch::record_own_devices(identity, sender, &devices);
        }
        // Handled by the session itself (`control_flow.rs`), which holds the keys they move.
        ControlMessage::Grant { .. } | ControlMessage::Epoch { .. } => {}
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
