//! Which peers this device has a sync session open with right now (task `sync-live-push`). A
//! session now stays open for as long as both ends are alive, so every dial loop (LAN resync,
//! relay auto-dial, a fresh mDNS sighting) asks here first and skips a peer that already has one:
//! the redial becomes a reconnect that only runs while no session is live.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, PoisonError};

use txtodo_model::DeviceId;

/// Cheap to clone: one `Arc<Mutex<_>>`. Counts sessions per peer, since a LAN and a relay session
/// to the same peer can briefly overlap.
#[derive(Clone, Default)]
pub(crate) struct LivePeers {
    sessions: Arc<Mutex<BTreeMap<DeviceId, usize>>>,
}

impl LivePeers {
    /// Marks `peer` live until the returned guard drops (the session ends).
    pub(crate) fn enter(&self, peer: DeviceId) -> LiveGuard {
        *self.lock().entry(peer).or_insert(0) += 1;
        LiveGuard {
            peers: self.clone(),
            peer,
        }
    }

    /// Whether a session with `peer` is open now.
    pub(crate) fn is_live(&self, peer: DeviceId) -> bool {
        self.lock().contains_key(&peer)
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, BTreeMap<DeviceId, usize>> {
        self.sessions.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// One live session's mark on [`LivePeers`]; dropping it un-marks the peer once its last session
/// ends.
pub(crate) struct LiveGuard {
    peers: LivePeers,
    peer: DeviceId,
}

impl Drop for LiveGuard {
    fn drop(&mut self) {
        let mut sessions = self.peers.lock();
        if let Some(count) = sessions.get_mut(&self.peer) {
            *count = count.saturating_sub(1);
            if *count == 0 {
                sessions.remove(&self.peer);
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
        let lan = peers.enter(peer);
        let relay = peers.enter(peer);
        drop(lan);
        assert!(peers.is_live(peer), "the relay session is still open");
        drop(relay);
        assert!(!peers.is_live(peer));
    }
}
