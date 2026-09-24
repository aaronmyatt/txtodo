//! Live LAN transport status (plan M4 `sync-lan-transport`), for `Health`/`txtodo doctor`. Owned
//! by the `Workspace` (not `lan.rs`) so `TxtodoService` needs no extra field to answer `Health`;
//! `lan.rs`'s background task updates it as it progresses. Split into its own file since it is
//! shared by both `workspace.rs` and `lan.rs`, neither of which should own the other's concern.
//! Relay fields (plan M8 `sync-relay-enable`, ADR 0026) added the same way: `relay.rs` (this
//! crate's daemon-level twin of `txtodo_sync::relay`, not that module itself) is the only writer.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

/// Cheap to clone: `Arc<AtomicBool>`s and `Arc<Mutex<String>>`s. Every workspace starts with
/// nothing configured/active; `lan.rs`/`relay.rs` flip these once the corresponding step actually
/// succeeds, never optimistically.
#[derive(Clone, Default)]
pub struct LanStatus {
    endpoint_bound: Arc<AtomicBool>,
    discovery_active: Arc<AtomicBool>,
    relay_configured: Arc<AtomicBool>,
    relay_url: Arc<Mutex<String>>,
    relay_last_outcome: Arc<Mutex<String>>,
    /// See [`LanStatus::offers_problem`]: the message and the unix ms it was seen.
    offers_problem: Arc<Mutex<Option<(String, u64)>>>,
}

impl LanStatus {
    /// Whether the local `iroh` endpoint bound successfully and is accepting connections.
    pub fn endpoint_bound(&self) -> bool {
        self.endpoint_bound.load(Ordering::Relaxed)
    }

    /// Whether mDNS is advertising and browsing for this workspace's sync group.
    pub fn discovery_active(&self) -> bool {
        self.discovery_active.load(Ordering::Relaxed)
    }

    /// True iff no relay is configured (plan M8 `sync-relay-enable` / ADR 0026: relay is an
    /// additive fallback, LAN stays primary). This being `false` is normal and expected once a
    /// human sets `--relay`/`relay_url` — never the failure the old `RELAY_DISABLED` constant's
    /// name implied under ADR 0024's relay-only world.
    pub fn relay_disabled(&self) -> bool {
        !self.relay_configured.load(Ordering::Relaxed)
    }

    /// The configured relay URL; empty when relay is off.
    pub fn relay_url(&self) -> String {
        self.relay_url
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// Human-readable outcome of the most recent relay bind/accept/connect attempt; empty until
    /// one has actually happened.
    pub fn relay_last_outcome(&self) -> String {
        self.relay_last_outcome
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    pub(crate) fn set_endpoint_bound(&self, value: bool) {
        self.endpoint_bound.store(value, Ordering::Relaxed);
    }

    pub(crate) fn set_discovery_active(&self, value: bool) {
        self.discovery_active.store(value, Ordering::Relaxed);
    }

    /// Records the configured relay URL; empty means relay is off (`relay.rs::start` calls this
    /// with `""` when `--relay` was never given, skipping a bind entirely).
    pub(crate) fn set_relay_configured(&self, url: &str) {
        self.relay_configured
            .store(!url.is_empty(), Ordering::Relaxed);
        *self
            .relay_url
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = url.to_owned();
    }

    /// Records the outcome of the most recent relay bind/accept/connect attempt.
    pub(crate) fn set_relay_last_outcome(&self, outcome: impl Into<String>) {
        *self
            .relay_last_outcome
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = outcome.into();
    }

    /// Why the last workspace-offer (control channel) session could not run, and when (task
    /// `control-channel-keystore-visibility`); `None` once a session read the group key again.
    /// Offers stop while it is set, so an empty offer list means "blocked", not "nothing offered".
    pub fn offers_problem(&self) -> Option<(String, u64)> {
        self.offers_problem
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// Records (or, with `None`, clears) [`Self::offers_problem`].
    pub(crate) fn set_offers_problem(&self, problem: Option<(String, u64)>) {
        *self
            .offers_problem
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = problem;
    }
}
