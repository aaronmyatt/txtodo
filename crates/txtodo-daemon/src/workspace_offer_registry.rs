//! Pending workspace offers this device has received but not yet accepted or declined (task
//! `daemon-workspace-identity-agreement` stage 4) — the receiving side's bookkeeping for a
//! `txtodo_sync::ControlMessage::Offer`. Mirrors `pairing_state.rs`'s `PairingRegistry` shape (a
//! `Mutex`-guarded inner state, capacity-checked inserts) but deliberately does **not** mirror its
//! single-slot cap: pairing has exactly one live handshake at a time, but a device-set has many
//! devices and many workspaces, so this registry supports many pending offers at once, bounded by
//! [`MAX_PENDING_OFFERS`] instead of a slot of one.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use txtodo_model::DeviceId;
use txtodo_store::WorkspaceId;

/// Most pending offers this device holds at once, across every peer — a hostile or buggy peer
/// flooding offers must not grow this without limit (every collection in this crate has a named,
/// checked cap).
pub const MAX_PENDING_OFFERS: usize = 256;

/// One workspace a peer has offered, not yet accepted or declined.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PendingOffer {
    /// The device that sent the offer.
    pub offering_device: DeviceId,
    /// The offered workspace's id, owned by `offering_device` (first-registrant-wins).
    pub workspace_id: WorkspaceId,
    /// Human-readable name, for an accept prompt only — never used as identity.
    pub name: String,
    /// The offering device's own clock reading when it sent the offer, milliseconds.
    pub offered_at_ms: u64,
}

/// Why recording a pending offer failed.
#[derive(Debug)]
pub enum WorkspaceOfferRegistryError {
    /// A genuinely new offer would exceed [`MAX_PENDING_OFFERS`]; a re-announce of an
    /// already-pending `(offering_device, workspace_id)` pair never hits this (see
    /// [`WorkspaceOfferRegistry::record`]'s doc).
    TooManyPending,
}

impl fmt::Display for WorkspaceOfferRegistryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WorkspaceOfferRegistryError::TooManyPending => {
                write!(f, "already holding {MAX_PENDING_OFFERS} pending offers")
            }
        }
    }
}

impl std::error::Error for WorkspaceOfferRegistryError {}

#[derive(Default)]
struct Inner {
    pending: BTreeMap<(DeviceId, WorkspaceId), PendingOffer>,
    /// Offers a human declined (task `remote-workspace-mirror`): a peer re-offers every workspace
    /// on every control session, so without this a declined one would come straight back and be
    /// mirrored. In memory only; removing a mirror is the durable "not this one".
    declined: BTreeSet<(DeviceId, WorkspaceId)>,
    /// When each pair was last offered, on this device's monotonic clock (task sync-drift line 8).
    /// `pending` is drained by the mirror task within a moment, so this is what "a paired device
    /// offers this workspace" reads. Declined pairs count too. Same cap; the oldest pair goes.
    seen: BTreeMap<(DeviceId, WorkspaceId), Instant>,
}

impl Inner {
    fn note_seen(&mut self, key: (DeviceId, WorkspaceId)) {
        if !self.seen.contains_key(&key) && self.seen.len() >= MAX_PENDING_OFFERS {
            let oldest = self.seen.iter().min_by_key(|(_, at)| **at).map(|(k, _)| *k);
            if let Some(oldest) = oldest {
                self.seen.remove(&oldest);
            }
        }
        self.seen.insert(key, Instant::now());
    }
}

/// This device's pending-offers bookkeeping. One per `DeviceIdentity` (device-level, not per
/// workspace) — the always-on control channel (stage 5) records offers here as they arrive; a
/// gRPC surface (stage 6) lists and consumes them.
#[derive(Default)]
pub struct WorkspaceOfferRegistry {
    inner: Mutex<Inner>,
    /// Woken on every recorded offer, so the catalog's mirror task (task `remote-workspace-mirror`)
    /// runs without polling. `notify_one` keeps one permit when nobody waits yet, so an offer
    /// recorded before the task starts is not missed.
    /// Ref: https://docs.rs/tokio/latest/tokio/sync/struct.Notify.html#method.notify_one
    wake: tokio::sync::Notify,
}

impl WorkspaceOfferRegistry {
    /// A fresh, empty registry.
    pub fn new() -> WorkspaceOfferRegistry {
        WorkspaceOfferRegistry::default()
    }

    /// Records `offer` as pending. Idempotent-in-place for a re-announce of the same
    /// `(offering_device, workspace_id)` pair (the control channel's own outbound loop
    /// idempotently re-offers this device's active workspaces every tick, stage 5's design) —
    /// only a genuinely *new* pair counts against [`MAX_PENDING_OFFERS`].
    ///
    /// A declined pair is ignored (`Ok`, nothing recorded).
    pub fn record(&self, offer: PendingOffer) -> Result<(), WorkspaceOfferRegistryError> {
        {
            let mut inner = self.lock();
            let key = (offer.offering_device, offer.workspace_id);
            inner.note_seen(key);
            if inner.declined.contains(&key) {
                return Ok(());
            }
            if !inner.pending.contains_key(&key) && inner.pending.len() >= MAX_PENDING_OFFERS {
                return Err(WorkspaceOfferRegistryError::TooManyPending);
            }
            inner.pending.insert(key, offer);
        }
        self.wake.notify_one();
        Ok(())
    }

    /// Consumes the pending offer and remembers the pair as declined, so a re-offer is ignored.
    /// `false` when no such offer was pending (nothing is remembered then either).
    pub fn decline(&self, offering_device: DeviceId, workspace_id: WorkspaceId) -> bool {
        let mut inner = self.lock();
        let key = (offering_device, workspace_id);
        if inner.pending.remove(&key).is_none() {
            return false;
        }
        if inner.declined.len() < MAX_PENDING_OFFERS {
            inner.declined.insert(key);
        }
        true
    }

    /// Resolves once an offer has been recorded since the last call returned.
    pub async fn recorded(&self) {
        self.wake.notified().await;
    }

    /// The devices that offered `workspace` within the last `within`, newest first (task
    /// sync-drift line 8: a rejoin needs one, or nothing would refill the folder).
    pub fn offered_by(&self, workspace: WorkspaceId, within: Duration) -> Vec<DeviceId> {
        let mut hits: Vec<(Instant, DeviceId)> = self
            .lock()
            .seen
            .iter()
            .filter(|((_, w), at)| *w == workspace && at.elapsed() <= within)
            .map(|((device, _), at)| (*at, *device))
            .collect();
        // Newest first. Ref: https://doc.rust-lang.org/std/cmp/struct.Reverse.html
        hits.sort_by_key(|(at, _)| std::cmp::Reverse(*at));
        hits.into_iter().map(|(_, device)| device).collect()
    }

    /// Every pending offer, for a listing RPC (stage 6). No particular order guaranteed beyond
    /// `BTreeMap`'s own `(DeviceId, WorkspaceId)` ordering.
    pub fn list(&self) -> Vec<PendingOffer> {
        self.lock().pending.values().cloned().collect()
    }

    /// Removes and returns the pending offer for `(offering_device, workspace_id)`, if any —
    /// consumed on accept or decline (stage 6), never left lingering after either.
    pub fn take(
        &self,
        offering_device: DeviceId,
        workspace_id: WorkspaceId,
    ) -> Option<PendingOffer> {
        self.lock().pending.remove(&(offering_device, workspace_id))
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }
}
