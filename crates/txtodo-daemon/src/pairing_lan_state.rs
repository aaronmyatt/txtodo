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
use txtodo_sync::{DiscoveredPeer, LanEndpoint, Nonce};

/// The initiator's most recently produced sealed `PairingGrant`, cached so a joiner's retried
/// `JoinerHello` (its response to the first one may simply have been lost — the network is best
/// effort) still gets the grant resent even though `PairingRegistry::try_finalize_initiator`
/// already cleared its own active-pairing state on success (`pairing_state.rs`'s own doc on why
/// that clearing stays as-is: this cache is a network-layer retry concern, not that registry's).
struct FinalizedGrant {
    device: DeviceId,
    nonce: Nonce,
    sealed: Vec<u8>,
}

/// Cheap to clone: three `Arc<Mutex<_>>`s. Every workspace starts with all empty; `lan.rs`'s
/// background task and `pairing_lan.rs`'s relay driver fill them in as they progress (an endpoint
/// once bound, a sighting each time mDNS resolves one, a grant once this device finalizes as
/// initiator), the same "never optimistic" discipline `LanStatus` already follows.
#[derive(Clone, Default)]
pub(crate) struct PairingLan {
    endpoint: Arc<Mutex<Option<Arc<LanEndpoint>>>>,
    sightings: Arc<Mutex<BTreeMap<DeviceId, DiscoveredPeer>>>,
    finalized: Arc<Mutex<Option<FinalizedGrant>>>,
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

    /// Caches a just-produced sealed grant, keyed by the joiner's device and the offer's nonce.
    pub(crate) fn cache_grant(&self, device: DeviceId, nonce: Nonce, sealed: Vec<u8>) {
        *self
            .finalized
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(FinalizedGrant {
            device,
            nonce,
            sealed,
        });
    }

    /// The cached grant for `(device, nonce)`, if one was produced — lets a retried `JoinerHello`
    /// get the same grant resent even after `PairingRegistry`'s own active-pairing state has
    /// already been cleared by the finalize that produced it.
    pub(crate) fn cached_grant(&self, device: DeviceId, nonce: Nonce) -> Option<Vec<u8>> {
        let guard = self
            .finalized
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let cached = guard.as_ref()?;
        (cached.device == device && cached.nonce == nonce).then(|| cached.sealed.clone())
    }
}
