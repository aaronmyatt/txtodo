//! Which peers this device has a sync session open with right now (task `sync-live-push`). A
//! session now stays open for as long as both ends are alive, so every dial loop (LAN resync,
//! relay auto-dial, a fresh mDNS sighting) asks here first and skips a peer that already has one:
//! the redial becomes a reconnect that only runs while no session is live.
//!
//! Each session says which carrier it runs over (task lan-dial-falls-to-relay, 2026-09-25). A peer
//! live only over the relay is still dialed over LAN, and once a LAN session with it is up the
//! relay one ends (`lan_session_live.rs`): long-lived sessions used to keep two Macs on one LAN
//! talking through the relay for as long as the relay session lasted.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, PoisonError};

use txtodo_model::DeviceId;

/// What a sync session runs over.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Carrier {
    /// A direct connection on the local network (`LanEndpoint`).
    Lan,
    /// Through the relay (`DeviceRelay`), or LAN's relay fallback.
    Relay,
}

/// Most dialed ids [`LivePeers::note_answered_as`] keeps: one per stale `devices` row, so a
/// handful in practice (one per device that changed identity since we paired with it).
const MAX_ANSWERED_AS: usize = 1_024;

/// Cheap to clone: two `Arc<Mutex<_>>`s. Counts sessions per peer and carrier, since a LAN and a
/// relay session to the same peer can briefly overlap.
#[derive(Clone, Default)]
pub(crate) struct LivePeers {
    sessions: Arc<Mutex<BTreeMap<(DeviceId, Carrier), usize>>>,
    /// Dialed id → the id whose `Hello` answered (task lan-dial-falls-to-relay, the 15 s relay
    /// restarts). A `devices` row can outlive its device's identity and still carry its relay node
    /// id: the dial reaches the device under its new id, so "is the dialed id live" never holds.
    answered_as: Arc<Mutex<BTreeMap<DeviceId, DeviceId>>>,
}

impl LivePeers {
    /// Marks `peer` live over `carrier` until the returned guard drops (the session ends).
    pub(crate) fn enter(&self, peer: DeviceId, carrier: Carrier) -> LiveGuard {
        *self.lock().entry((peer, carrier)).or_insert(0) += 1;
        LiveGuard {
            peers: self.clone(),
            key: (peer, carrier),
        }
    }

    /// Whether a session with `peer` is open now, over any carrier.
    pub(crate) fn is_live(&self, peer: DeviceId) -> bool {
        self.is_live_on(peer, Carrier::Lan) || self.is_live_on(peer, Carrier::Relay)
    }

    /// Whether a session with `peer` is open now over `carrier`.
    pub(crate) fn is_live_on(&self, peer: DeviceId, carrier: Carrier) -> bool {
        self.lock().contains_key(&(peer, carrier))
    }

    /// Records that a dial aimed at `dialed` was answered by `greeted`. `true` the first time this
    /// answer is seen, so the caller warns once. Full and new: not kept (logged by the caller as
    /// usual; the dial then simply repeats, as before this map).
    pub(crate) fn note_answered_as(&self, dialed: DeviceId, greeted: DeviceId) -> bool {
        let mut answered = self
            .answered_as
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if answered.len() >= MAX_ANSWERED_AS && !answered.contains_key(&dialed) {
            return false;
        }
        answered.insert(dialed, greeted) != Some(greeted)
    }

    /// [`Self::is_live`] for a dial target: also true when `dialed` last answered as a device
    /// that is live now.
    pub(crate) fn is_live_as_dialed(&self, dialed: DeviceId) -> bool {
        let greeted = self
            .answered_as
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&dialed)
            .copied();
        self.is_live(dialed) || greeted.is_some_and(|g| self.is_live(g))
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, BTreeMap<(DeviceId, Carrier), usize>> {
        self.sessions.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// One live session's mark on [`LivePeers`]; dropping it un-marks the peer on that carrier once
/// its last session there ends.
pub(crate) struct LiveGuard {
    peers: LivePeers,
    key: (DeviceId, Carrier),
}

impl LiveGuard {
    /// The peer and carrier this session marked.
    pub(crate) fn key(&self) -> (DeviceId, Carrier) {
        self.key
    }

    /// The set this session is counted in.
    pub(crate) fn peers(&self) -> &LivePeers {
        &self.peers
    }
}

impl Drop for LiveGuard {
    fn drop(&mut self) {
        let mut sessions = self.peers.lock();
        if let Some(count) = sessions.get_mut(&self.key) {
            *count = count.saturating_sub(1);
            if *count == 0 {
                sessions.remove(&self.key);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use txtodo_model::Ulid;

    #[test]
    fn a_peer_is_live_until_its_last_session_ends() {
        let peers = LivePeers::default();
        let peer = DeviceId::new(Ulid::from_u128(7));
        assert!(!peers.is_live(peer));
        let lan = peers.enter(peer, Carrier::Lan);
        let relay = peers.enter(peer, Carrier::Relay);
        drop(lan);
        assert!(peers.is_live(peer), "the relay session is still open");
        assert!(!peers.is_live_on(peer, Carrier::Lan));
        assert!(peers.is_live_on(peer, Carrier::Relay));
        drop(relay);
        assert!(!peers.is_live(peer));
    }

    /// The 15 s relay restarts: a dial to a stale row's id reached a peer live under another id.
    #[test]
    fn a_dialed_id_that_answered_as_a_live_peer_counts_as_live() {
        let peers = LivePeers::default();
        let (stale, real) = (
            DeviceId::new(Ulid::from_u128(7)),
            DeviceId::new(Ulid::from_u128(8)),
        );
        let lan = peers.enter(real, Carrier::Lan);
        assert!(
            !peers.is_live_as_dialed(stale),
            "nothing links the two ids yet"
        );
        assert!(peers.note_answered_as(stale, real), "first sighting warns");
        assert!(
            !peers.note_answered_as(stale, real),
            "the same answer again does not"
        );
        assert!(peers.is_live_as_dialed(stale));
        drop(lan);
        assert!(
            !peers.is_live_as_dialed(stale),
            "dialed again once the real one is gone"
        );
    }

    #[test]
    fn answered_as_is_bounded() {
        let peers = LivePeers::default();
        let real = DeviceId::new(Ulid::from_u128(1));
        for n in 0..MAX_ANSWERED_AS as u128 {
            peers.note_answered_as(DeviceId::new(Ulid::from_u128(100 + n)), real);
        }
        let one_more = DeviceId::new(Ulid::from_u128(99));
        assert!(!peers.note_answered_as(one_more, real));
        let _lan = peers.enter(real, Carrier::Lan);
        assert!(!peers.is_live_as_dialed(one_more));
        assert!(peers.is_live_as_dialed(DeviceId::new(Ulid::from_u128(100))));
    }
}
