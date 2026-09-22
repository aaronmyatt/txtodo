//! Shared, cheap-to-clone state connecting `lan.rs`'s background LAN transport task to the
//! pairing gRPC handlers (`pairing_grpc.rs`) and the pairing relay driver (`pairing_lan.rs`): the
//! bound `LanEndpoint` (once `lan.rs` binds one) and every raw mDNS sighting this device has ever
//! resolved, regardless of sync group. Pairing needs to find a peer whose group is, by definition,
//! not yet this device's own — the normal group-filtered `PeerTable`/`lan_peers::KnownPeers` exist
//! precisely to *exclude* that, so pairing cannot reuse either and needs this separate, unfiltered
//! lookup instead. Owned by `Workspace`, same pattern as `lan_status.rs`'s `LanStatus`.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, PoisonError};

use txtodo_model::DeviceId;
use txtodo_sync::{DiscoveredPeer, LanEndpoint};

/// Cheap to clone: three `Arc<Mutex<_>>`s. Every workspace starts with all empty; `lan.rs`'s
/// background task and `pairing_lan.rs`'s relay driver fill them in as they progress (an endpoint
/// once bound, a sighting each time mDNS resolves one), the same "never optimistic" discipline
/// `LanStatus` already follows. The sealed grant is cached device-globally on `PairingRegistry`
/// instead (`pairing_state.rs`), not here — see that field's doc for why per-workspace was wrong.
#[derive(Clone, Default)]
pub(crate) struct PairingLan {
    endpoint: Arc<Mutex<Option<Arc<LanEndpoint>>>>,
    sightings: Arc<Mutex<BTreeMap<DeviceId, DiscoveredPeer>>>,
    /// Which carrier ("lan" or "relay") the most recently *completed* pairing actually used (plan
    /// M8 `sync-pairing-relay`), for `Health.pairing_last_carrier`/`txtodo doctor` — empty until a
    /// pairing has finished on this device at all, never optimistic.
    carrier: Arc<Mutex<String>>,
}

impl PairingLan {
    /// Records the endpoint `lan.rs` bound, so a pairing dial (this device as joiner) can reuse it
    /// rather than binding a second one.
    pub(crate) fn set_endpoint(&self, endpoint: Arc<LanEndpoint>) {
        *self.endpoint.lock().unwrap_or_else(PoisonError::into_inner) = Some(endpoint);
    }

    /// The bound endpoint, if `lan.rs` has bound one yet (a bind failure leaves this `None`
    /// forever for this process — see `lan.rs`'s own "never fatal" doc).
    pub(crate) fn endpoint(&self) -> Option<Arc<LanEndpoint>> {
        self.endpoint
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// Remembers a resolved sighting regardless of its group — unlike `lan_peers::KnownPeers`,
    /// which only ever holds sightings that already passed `PeerTable`'s self/foreign-group filter.
    pub(crate) fn remember(&self, peer: &DiscoveredPeer) {
        self.sightings
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(peer.device, peer.clone());
    }

    /// The most recent sighting of `device`, if this daemon has ever resolved one — for the joiner
    /// side of pairing to find the initiator's address before dialing it.
    pub(crate) fn find(&self, device: DeviceId) -> Option<DiscoveredPeer> {
        self.sightings
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&device)
            .cloned()
    }

    /// Records `carrier` ("lan" or "relay") as the carrier the most recently completed pairing
    /// used — called only when a round actually finalizes (an `InitiatorReply::Grant`), never on
    /// `Pending`/`Rejected`, by both the initiator's `handle_incoming_over` and the joiner's
    /// `pairing_relay_dial::joiner_round`.
    pub(crate) fn record_carrier(&self, carrier: &'static str) {
        *self.carrier.lock().unwrap_or_else(PoisonError::into_inner) = carrier.to_owned();
    }

    /// The carrier the most recently completed pairing used; empty until one has finished.
    pub(crate) fn carrier(&self) -> String {
        self.carrier
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}
