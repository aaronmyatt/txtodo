//! Peer bookkeeping for `lan.rs`: the dial tie-break/backoff decision (`DialState`) and the set of
//! every peer this device has ever resolved for real (`KnownPeers`, `lan.rs`'s periodic redial).
//! Split out of `lan.rs` purely for the file budget.

use std::collections::{BTreeMap, BTreeSet};
use std::net::SocketAddr;
use std::sync::{Arc, Mutex, PoisonError};

use txtodo_model::DeviceId;
use txtodo_sync::{
    DiscoveredPeer, GroupId, PROTOCOL_VERSION, PeerEvent, PeerTable, Sighting, backoff_ms,
};

/// Shared across the run loop and every spawned dial task.
pub(crate) type SharedDialState = Arc<Mutex<DialState>>;
/// Every peer this device has ever resolved for real, kept for `lan.rs`'s `redial_known_peers`.
pub(crate) type KnownPeers = Arc<Mutex<BTreeMap<DeviceId, DiscoveredPeer>>>;

/// Attempt/backoff bookkeeping per peer: when we last dialed, and how many failures in a row.
#[derive(Default)]
pub(crate) struct DialState {
    last_attempt_ms: BTreeMap<DeviceId, u64>,
    failures: BTreeMap<DeviceId, u32>,
}

impl DialState {
    /// Whether enough time has passed since the last attempt at `peer`, per `backoff_ms`.
    fn due(&self, peer: DeviceId, now_ms: u64) -> bool {
        match self.last_attempt_ms.get(&peer) {
            None => true,
            Some(&last) => {
                let attempt = self.failures.get(&peer).copied().unwrap_or(0);
                now_ms.saturating_sub(last) >= backoff_ms(attempt)
            }
        }
    }

    fn record_attempt(&mut self, peer: DeviceId, now_ms: u64) {
        self.last_attempt_ms.insert(peer, now_ms);
    }

    fn record_failure(&mut self, peer: DeviceId) {
        *self.failures.entry(peer).or_insert(0) += 1;
    }

    fn record_success(&mut self, peer: DeviceId) {
        self.failures.remove(&peer);
    }
}

/// Logged for every real sighting, dialed or not — the only externally observable (via the JSON
/// log) proof that discovery itself worked, independent of whether the connect step that follows
/// succeeds. `tests/lan_discovery.rs` polls for exactly this line.
fn log_peer_found(peer: &DiscoveredPeer) {
    tracing::info!(peer = %peer.device, addresses = ?peer.addresses, "lan_peer_found");
}

/// Records `sighting` in `pairing_lan()`'s unfiltered address book, regardless of which group it
/// claims — see `lan.rs::handle_sighting`'s call site and `pairing_lan_state.rs`'s module doc.
pub(crate) fn remember_any_sighting(
    pairing: &crate::pairing_lan_state::PairingLan,
    sighting: &Sighting,
) {
    let peer = DiscoveredPeer {
        device: sighting.announcement.device,
        node: sighting.announcement.node,
        addresses: sighting.addresses.clone(),
    };
    pairing.remember(&peer);
}

/// Most addresses remembered per peer, however many answers mDNS merges.
const MAX_PEER_ADDRESSES: usize = 32;

/// Every in-group sighting of a peer this device dials (the lower id, the tie-break) keeps its
/// remembered addresses current, whatever the debounce says (task lan-dial-falls-to-relay,
/// 2026-09-25). Only a `Found` sighting used to be remembered, so the first answer, sometimes
/// IPv6-only, was the address set every redial used. The higher id still remembers nothing: it
/// is the side that dials a peer over the relay when the peer leaves the LAN
/// (`relay_autodial::relay_only_peers`), and a LAN session supersedes that relay one.
pub(crate) fn remember_sighting(
    known_peers: &KnownPeers,
    sighting: &Sighting,
    own: DeviceId,
    group: GroupId,
) {
    let a = &sighting.announcement;
    if own >= a.device || a.group != group || a.proto != PROTOCOL_VERSION {
        return;
    }
    let mut known = known_peers.lock().unwrap_or_else(PoisonError::into_inner);
    let addresses = match known.get(&a.device) {
        Some(old) if old.node == a.node => merge_addresses(&old.addresses, &sighting.addresses),
        _ => sighting.addresses.clone(),
    };
    let peer = DiscoveredPeer {
        device: a.device,
        node: a.node,
        addresses,
    };
    known.insert(a.device, peer);
}

/// `new`, then each of `old` on a port `new` uses that `new` lacks: mDNS can answer a peer's IPv4
/// and IPv6 addresses separately, and a later answer must not drop an earlier one. A new port is a
/// restarted peer, so its old addresses go.
fn merge_addresses(old: &[SocketAddr], new: &[SocketAddr]) -> Vec<SocketAddr> {
    let ports: BTreeSet<u16> = new.iter().map(SocketAddr::port).collect();
    let mut out = new.to_vec();
    for addr in old {
        if ports.contains(&addr.port()) && !out.contains(addr) {
            out.push(*addr);
        }
    }
    out.truncate(MAX_PEER_ADDRESSES);
    debug_assert!(out.len() <= MAX_PEER_ADDRESSES);
    out
}

/// `peer` as remembered, addresses merged across sightings, or `peer` itself when it is not.
pub(crate) fn known_or(known_peers: &KnownPeers, peer: DiscoveredPeer) -> DiscoveredPeer {
    let known = known_peers.lock().unwrap_or_else(PoisonError::into_inner);
    known.get(&peer.device).cloned().unwrap_or(peer)
}

/// Every currently known peer this device (rather than the peer) is responsible for dialing —
/// `relay_autodial::resync_and_dial` calls this each tick and then gates each one through
/// [`try_begin_dial`], so a peer whose last dial failed is left alone until its backoff elapses.
pub(crate) fn peers_to_resync(known_peers: &KnownPeers, device: DeviceId) -> Vec<DiscoveredPeer> {
    known_peers
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .values()
        .filter(|p| device < p.device)
        .cloned()
        .collect()
}

/// `table.observe` plus the dial tie-break and backoff check, collapsed to one `Option`: `Some`
/// only when this device should actually dial `peer` right now.
pub(crate) fn worth_dialing(
    sighting: Sighting,
    table: &mut PeerTable,
    dial_state: &SharedDialState,
    now_ms: u64,
    device: DeviceId,
) -> Option<DiscoveredPeer> {
    let PeerEvent::Found(peer) = table.observe(sighting.announcement, sighting.addresses, now_ms)
    else {
        return None;
    };
    log_peer_found(&peer);
    let mut dial_state = dial_state.lock().unwrap_or_else(PoisonError::into_inner);
    // Tie-break: only the lower device id dials, so two daemons that discover each other at the
    // same moment never open two redundant connections.
    if device >= peer.device || !dial_state.due(peer.device, now_ms) {
        return None;
    }
    dial_state.record_attempt(peer.device, now_ms);
    Some(peer)
}

/// The resync tick's gate: `true` (and the attempt booked) when `peer`'s backoff has elapsed,
/// `false` to leave it alone this tick. Same `DialState` as the sighting path, so a failed dial
/// from either side pushes the next one out by `backoff_ms`.
pub(crate) fn try_begin_dial(dial_state: &SharedDialState, peer: DeviceId, now_ms: u64) -> bool {
    let mut dial_state = dial_state.lock().unwrap_or_else(PoisonError::into_inner);
    if !dial_state.due(peer, now_ms) {
        return false;
    }
    dial_state.record_attempt(peer, now_ms);
    true
}

/// Books how a dial went. `ok` is false both for a connect that never happened and for one whose
/// session bailed before its first greeting (`lan.rs::dial_and_spawn`), so both back off.
pub(crate) fn record_dial_outcome(dial_state: &SharedDialState, peer: DeviceId, ok: bool) {
    let mut dial_state = dial_state.lock().unwrap_or_else(PoisonError::into_inner);
    if ok {
        dial_state.record_success(peer);
    } else {
        dial_state.record_failure(peer);
    }
}
