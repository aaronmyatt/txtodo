//! Relay-only auto-dial for a paired device this daemon has never seen on the LAN (root todo
//! `sync-pairing-relay-ongoing-dial`): `pairing_lan.rs::finish_joiner` already records a peer's
//! relay node id in the devices table once pairing completes (`Workspace::
//! record_peer_relay_reachability`), but until now nothing read it back — `lan.rs`'s own
//! LAN-then-relay fallback (`relay_fallback.rs`) only ever runs for a `DiscoveredPeer`, which by
//! construction never exists for a peer that shares no LAN with this device at all. Split out of
//! `lan.rs`, which is already at the file-length budget.

use std::collections::BTreeMap;
use std::sync::{Arc, PoisonError};

use tokio::sync::Semaphore;
use txtodo_model::DeviceId;
use txtodo_store::DeviceRow;
use txtodo_sync::{DiscoveredPeer, LanEndpoint};

use crate::lan::{LanCtx, dial_and_spawn, spawn_driver};
use crate::lan_peers::{KnownPeers, SharedDialState, peers_to_resync, try_begin_dial};
use crate::live_peers::Carrier;
use crate::relay_fallback::relay_fallback_dial;

/// Runs both halves of a resync tick: `lan.rs`'s existing known-peer redial, then this module's
/// relay-only auto-dial for peers with no LAN sighting at all. Collapsed to one call so `lan.rs`
/// stays under its file-length budget.
pub(crate) fn resync_and_dial(
    known_peers: &KnownPeers,
    ctx: &LanCtx,
    endpoint: &Arc<LanEndpoint>,
    sessions: &Arc<Semaphore>,
    dial_state: &SharedDialState,
) {
    // Only peers with no live LAN session (task sync-live-push): a session stays open now, so the
    // resync is a reconnect, not a redial of a link that is still up. One live only over the relay
    // is dialed over LAN, no relay fallback (task lan-dial-falls-to-relay, `dial_and_spawn`).
    let live = ctx.identity.live_peers();
    for peer in peers_to_resync(known_peers, ctx.device)
        .into_iter()
        .filter(|p| !live.is_live_on(p.device, Carrier::Lan))
    {
        spawn_resync_dial(
            Arc::clone(sessions),
            ctx.clone(),
            Arc::clone(endpoint),
            Arc::clone(dial_state),
            peer,
        );
    }
    for (device, node) in relay_only_peers(ctx, known_peers)
        .into_iter()
        .filter(|(device, _)| !live.is_live(*device))
    {
        spawn_relay_only_dial(ctx.clone(), node, device, Arc::clone(sessions));
    }
}

/// Every registered, non-removed device with a recorded relay node id that `known_peers` has
/// never resolved on the LAN — `peers_to_resync`'s relay-only counterpart. Reads the devices table
/// and `known_peers`, then delegates the actual decision to [`filter_relay_only`] so that decision
/// stays unit-testable without a real `Workspace`.
fn relay_only_peers(ctx: &LanCtx, known_peers: &KnownPeers) -> Vec<(DeviceId, [u8; 32])> {
    let rows = ctx
        .identity
        .store()
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .list_devices()
        .unwrap_or_default();
    let known = known_peers.lock().unwrap_or_else(PoisonError::into_inner);
    filter_relay_only(rows, &known)
}

/// Pure decision: which `rows` are worth a relay-only auto-dial right now. A row qualifies when
/// it is not removed, carries a recorded relay node id, and is not already in `known` (a real LAN
/// sighting always wins — this path is only for a peer with none).
///
/// **Deliberately no `peers_to_resync`-style device-id tie-break.** That tie-break only makes
/// sense when both sides hold symmetric knowledge (any two mDNS-sighted peers both learn of each
/// other), so it just avoids a wasteful duplicate dial. `record_peer_relay_reachability` is
/// one-directional today (`pairing_lan.rs::finish_joiner`: only the joiner records the
/// initiator's relay node id, never the reverse — a documented gap, not this task's job) — a
/// device-id tie-break here could silence the *only* side that ever has the data to dial with,
/// leaving the pair permanently un-synced whenever the joiner's id happens to sort higher. A
/// harmless double connection if both sides ever do hold each other's reachability one day beats
/// that.
fn filter_relay_only(
    rows: Vec<DeviceRow>,
    known: &BTreeMap<DeviceId, DiscoveredPeer>,
) -> Vec<(DeviceId, [u8; 32])> {
    rows.into_iter()
        .filter(|d| d.removed_at_ms.is_none() && !known.contains_key(&d.device))
        .filter_map(|d| d.relay_node_id.map(|node| (d.device, node)))
        .collect()
}

/// The periodic-resync counterpart of `lan.rs`'s `spawn_dial`: the same connect-and-drive, gated
/// by the same `DialState` backoff (`try_begin_dial`) — until 2026-09-23 this was unconditional
/// churn, one dial per known peer per tick regardless of how the last one went.
fn spawn_resync_dial(
    sessions: Arc<Semaphore>,
    ctx: LanCtx,
    endpoint: Arc<LanEndpoint>,
    dial_state: SharedDialState,
    peer: DiscoveredPeer,
) {
    let Some(permit) = resync_permit(&sessions, &ctx, &dial_state, peer.device) else {
        return;
    };
    tokio::spawn(async move {
        let device = peer.device;
        let ok = dial_and_spawn(ctx, endpoint, peer, permit, dial_state).await;
        tracing::debug!(peer = %device, ok, "lan_resync_dial_outcome");
    });
}

/// The backoff gate, then the session cap: `None` (already logged) skips `peer` this tick.
fn resync_permit(
    sessions: &Arc<Semaphore>,
    ctx: &LanCtx,
    dial_state: &SharedDialState,
    peer: DeviceId,
) -> Option<tokio::sync::OwnedSemaphorePermit> {
    let now_ms = ctx.clock.now_ms();
    if !try_begin_dial(dial_state, peer, now_ms) {
        return log_backing_off(peer);
    }
    Arc::clone(sessions)
        .try_acquire_owned()
        .ok()
        .or_else(|| log_cap_reached(peer))
}

fn log_backing_off(peer: DeviceId) -> Option<tokio::sync::OwnedSemaphorePermit> {
    tracing::debug!(%peer, "lan_resync_dial_backing_off");
    None
}

fn log_cap_reached(peer: DeviceId) -> Option<tokio::sync::OwnedSemaphorePermit> {
    tracing::debug!(%peer, "lan_session_cap_reached_skipping_resync");
    None
}

/// One relay-only dial attempt, bounded by `sessions` the same as every other dial in this crate.
fn spawn_relay_only_dial(ctx: LanCtx, node: [u8; 32], device: DeviceId, sessions: Arc<Semaphore>) {
    let Ok(permit) = sessions.try_acquire_owned() else {
        return;
    };
    tokio::spawn(async move {
        match relay_fallback_dial(ctx.clone(), node).await {
            Some(link) => spawn_driver(ctx, (link, Carrier::Relay), permit, |_| {}),
            None => tracing::debug!(peer = %device, "relay_only_auto_dial_failed"),
        }
    });
}

#[cfg(test)]
mod tests {
    use super::filter_relay_only;
    use std::collections::BTreeMap;
    use txtodo_model::{DeviceId, Ulid};
    use txtodo_store::DeviceRow;

    fn device(n: u128) -> DeviceId {
        DeviceId::new(Ulid::from_u128(n))
    }

    fn row(device: DeviceId, relay_node_id: Option<[u8; 32]>, removed: bool) -> DeviceRow {
        DeviceRow {
            device,
            name: String::new(),
            static_public: [0; txtodo_store::DEVICE_STATIC_KEY_BYTES],
            paired_at_ms: 0,
            last_seen_ms: None,
            last_known_wall_ms: None,
            key_epoch: 0,
            removed_at_ms: removed.then_some(1),
            relay_node_id,
            relay_url: relay_node_id.map(|_| "relay:example".to_owned()),
        }
    }

    #[test]
    fn dials_a_relay_only_peer_with_no_lan_sighting() {
        let peer = device(2);
        let rows = vec![row(peer, Some([7; 32]), false)];
        let known = BTreeMap::new();
        assert_eq!(filter_relay_only(rows, &known), vec![(peer, [7; 32])]);
    }

    #[test]
    fn skips_a_peer_already_known_via_lan() {
        let peer = device(2);
        let rows = vec![row(peer, Some([7; 32]), false)];
        let mut known = BTreeMap::new();
        known.insert(peer, sample_discovered_peer(peer));
        assert!(filter_relay_only(rows, &known).is_empty());
    }

    #[test]
    fn skips_a_removed_device() {
        let peer = device(2);
        let rows = vec![row(peer, Some([7; 32]), true)];
        assert!(filter_relay_only(rows, &BTreeMap::new()).is_empty());
    }

    #[test]
    fn skips_a_peer_with_no_recorded_relay_node_id() {
        let peer = device(2);
        let rows = vec![row(peer, None, false)];
        assert!(filter_relay_only(rows, &BTreeMap::new()).is_empty());
    }

    fn sample_discovered_peer(device: DeviceId) -> txtodo_sync::DiscoveredPeer {
        txtodo_sync::DiscoveredPeer {
            device,
            node: [0; 32],
            addresses: Vec::new(),
        }
    }
}
