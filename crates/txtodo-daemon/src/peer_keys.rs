//! What sessions have shown about each peer's group key (task sync-drift line 5): whether its
//! frames open under ours, and which open failures were already logged. Device-wide and in memory
//! only, like `live_peers.rs`; a restart forgets it all.
//!
//! **Parking.** A peer whose frames fail to open with `wrong_group` [`PARK_AFTER`] times in a row
//! holds no key we share: it was paired once, then one of us joined another group. Every dial loop
//! (LAN resync and control, relay-only sync, relay control) skips a parked peer; its devices row is
//! left alone. It comes back when a pairing registers it, when it is sighted on the LAN in our
//! group, when a frame of its opens (it dialed us), when our own group changes, or on restart.
//!
//! **Another protocol** (task sync-divergence-check/protocol-mismatch). A peer whose frame or link
//! `Hello` carries another sync protocol than ours refuses us and we refuse it: nothing syncs until
//! the older device is upgraded. The protocol it spoke is kept per known peer and shown in
//! `SyncStatus` (`other_protocol`), so clients can say so; a session that greets clears it. Not
//! parked: the peer holds our key, and dialing it is how we learn it was upgraded.
//!
//! **Logging.** An open failure warns once per peer and kind per daemon run (`peer_open_failed`,
//! with the peer id); repeats go to debug. An incoming session's peer is not known until its
//! `Hello` opens, so the failures of incoming sessions share one `unknown` peer.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use txtodo_model::DeviceId;

/// `wrong_group` opens in a row after which a peer is parked. One can be a race with a pairing in
/// flight (the joiner switches group and re-advertises within a second; a dial in between sees the
/// old one). Two in a row, a backoff apart, is unlikely; three is not a race. Parking is cheap to
/// undo (module doc), so a small number costs little. A stale peer is booked about twice per resync
/// tick (a sync dial and a control dial), so it is parked within about 30 s.
pub(crate) const PARK_AFTER: u32 = 3;

/// Most peers tracked at once, the one `unknown` peer included: far more than one device-set has.
/// A new peer past it is not tracked (never parked, its failures logged at debug).
pub(crate) const MAX_TRACKED_PEERS: usize = 256;

/// The kind of an open failure for a frame sealed under another group's key
/// (`CryptoError::WrongGroup`, named in `lan_session_shared.rs`'s `crypto_error_kind`).
pub(crate) const WRONG_GROUP: &str = "wrong_group";

/// The kind of an open failure for a frame sealed under an epoch this device holds no key for
/// (`CryptoError::UnknownEpoch`): a peer ahead of us whose grant has not reached us, or, if it never
/// does, a group this device was dropped from.
pub(crate) const UNKNOWN_EPOCH: &str = "unknown_epoch";

/// `unknown_epoch` opens in a row, from anyone, after which this device calls itself dropped from
/// the group (ADR 0044). A grant normally arrives in the control session of the same resync tick, so
/// the run is one or two long when a peer is merely ahead.
pub(crate) const DROPPED_AFTER: u32 = 3;

/// The kind logged for a peer on another sync protocol.
pub(crate) const OTHER_PROTOCOL: &str = "other_protocol";

/// What one session showed about its peer's key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PeerSignal {
    /// A frame of the peer's opened under our group key.
    Opened,
    /// A frame of the peer's came and did not open: the failure's kind.
    OpenFailed(&'static str),
    /// The peer's frame or link `Hello` carried this sync protocol, not ours.
    OtherProtocol(u16),
    /// Nothing to go on: no frame came, or the session never started.
    Silent,
}

/// How a sync session (`lan_session_dispatch::drive_shared_session`) ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SessionEnd {
    /// The peer's link `Hello` opened and was accepted: a real session ran with this peer.
    Greeted(DeviceId),
    /// The peer's link `Hello` came and did not open: the failure's kind.
    Refused(&'static str),
    /// The peer's frame or link `Hello` carried this sync protocol, not ours.
    OtherProtocol(u16),
    /// No `Hello` of the peer's was accepted for another reason: nothing to send with, the link
    /// closed first, or the `Hello` opened but was refused (logged where it was refused).
    NoHello,
}

impl SessionEnd {
    /// From what the session saw: why the peer was refused, else the peer its accepted `Hello`
    /// named, else the other protocol it spoke, else why the `Hello` did not open.
    pub(crate) fn of(
        peer: Option<DeviceId>,
        refused: Option<&'static str>,
        other_protocol: Option<u16>,
    ) -> SessionEnd {
        match (peer, other_protocol, refused) {
            // A `Hello` that opened and was then refused (a removed device, `greet_for_peer`).
            (Some(_), _, Some(kind)) => SessionEnd::Refused(kind),
            (Some(peer), _, None) => SessionEnd::Greeted(peer),
            (None, Some(protocol), _) => SessionEnd::OtherProtocol(protocol),
            (None, None, Some(kind)) => SessionEnd::Refused(kind),
            (None, None, None) => SessionEnd::NoHello,
        }
    }

    /// Whether a dial counts as a success: only once the peer's `Hello` opened. It used to count
    /// once our own `Hello` was sent, so a peer with another group's key reset its backoff on every
    /// dial.
    pub(crate) fn greeted(self) -> bool {
        matches!(self, SessionEnd::Greeted(_))
    }

    fn signal(self) -> PeerSignal {
        match self {
            SessionEnd::Greeted(_) => PeerSignal::Opened,
            SessionEnd::Refused(kind) => PeerSignal::OpenFailed(kind),
            SessionEnd::OtherProtocol(protocol) => PeerSignal::OtherProtocol(protocol),
            SessionEnd::NoHello => PeerSignal::Silent,
        }
    }
}

/// One peer's record.
#[derive(Default)]
struct PeerKey {
    /// `wrong_group` opens in a row, with no other outcome in between.
    wrong_group: u32,
    /// Open-failure kinds already warned about this run: a closed set, a handful at most.
    warned: BTreeSet<&'static str>,
    /// The sync protocol the peer last spoke when it was not ours; `None` once a session greets.
    other_protocol: Option<u16>,
}

impl PeerKey {
    fn parked(&self) -> bool {
        self.wrong_group >= PARK_AFTER
    }
}

/// What [`PeerKeys::note_failure`] decided, for the caller to log.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Noted {
    /// The first failure of this kind for this peer this run: warn, not debug.
    pub(crate) first: bool,
    /// This failure parked the peer.
    pub(crate) parked_now: bool,
}

/// Cheap to clone: one `Arc<Mutex<_>>`, like `LivePeers`. Keyed by `Option<DeviceId>`: `None` is
/// the unknown peer of an incoming session whose `Hello` did not open.
#[derive(Clone, Default)]
pub(crate) struct PeerKeys {
    peers: Arc<Mutex<BTreeMap<Option<DeviceId>, PeerKey>>>,
    /// `unknown_epoch` opens in a row from any peer, ended by any frame opening, a new epoch or a
    /// new group: not per peer, since a device dropped from the group often cannot tell who sent it.
    epoch_misses: Arc<AtomicU32>,
    /// When the current run of `unknown_epoch` opens began: a run of any length that has lasted
    /// [`dropped_window`] without a grant, an opened frame or a new epoch is a drop too, since a
    /// dropped device is dialed only now and then and may see just one or two.
    first_miss: Arc<Mutex<Option<Instant>>>,
}

/// How long a run of `unknown_epoch` opens may last before it counts as dropped: a grant comes in
/// the control session of the same resync round, so two rounds without one is long enough.
pub(crate) fn dropped_window() -> Duration {
    (crate::lan::resync_interval() * 2).max(Duration::from_secs(5))
}

impl PeerKeys {
    /// Books what one session with `peer` (`None`: unknown) showed, and logs it. `channel` says
    /// which kind of session it was (`sync` or `control`).
    pub(crate) fn book(&self, peer: Option<DeviceId>, signal: PeerSignal, channel: &'static str) {
        match signal {
            PeerSignal::Silent => {}
            PeerSignal::Opened => {
                self.clear_epoch_misses();
                if let Some(peer) = peer {
                    self.forget(peer, "opened");
                    self.set_other_protocol(peer, None);
                }
            }
            PeerSignal::OtherProtocol(protocol) => {
                let noted = self.note_failure(peer, OTHER_PROTOCOL);
                log_open_failed(peer, OTHER_PROTOCOL, channel, noted.first);
                if let Some(peer) = peer {
                    self.set_other_protocol(peer, Some(protocol));
                }
            }
            PeerSignal::OpenFailed(kind) => {
                self.note_epoch_miss(kind);
                let noted = self.note_failure(peer, kind);
                log_open_failed(peer, kind, channel, noted.first);
                if noted.parked_now {
                    log_parked(peer);
                }
            }
        }
    }

    /// [`PeerKeys::book`] for a sync session: `dialed` is the peer the dialer meant to reach
    /// (`None` for an incoming session); a greeted session names its peer itself.
    pub(crate) fn book_session(&self, dialed: Option<DeviceId>, end: SessionEnd) {
        let peer = match end {
            SessionEnd::Greeted(peer) => Some(peer),
            SessionEnd::Refused(_) | SessionEnd::OtherProtocol(_) | SessionEnd::NoHello => dialed,
        };
        self.book(peer, end.signal(), "sync");
    }

    /// Counts one open failure of `kind` from `peer`. Any other kind ends a `wrong_group` run (most
    /// are checked after the group, so the peer sealed under ours). Only a known peer is parked.
    pub(crate) fn note_failure(&self, peer: Option<DeviceId>, kind: &'static str) -> Noted {
        let mut peers = self.lock();
        if !peers.contains_key(&peer) && peers.len() >= MAX_TRACKED_PEERS {
            return Noted {
                first: false,
                parked_now: false,
            };
        }
        let key = peers.entry(peer).or_default();
        // `insert` is true only when the kind was not there yet.
        // Ref: https://doc.rust-lang.org/std/collections/struct.BTreeSet.html#method.insert
        let first = key.warned.insert(kind);
        if kind != WRONG_GROUP || peer.is_none() {
            key.wrong_group = 0;
            return Noted {
                first,
                parked_now: false,
            };
        }
        key.wrong_group = key.wrong_group.saturating_add(1);
        Noted {
            first,
            parked_now: key.wrong_group == PARK_AFTER,
        }
    }

    /// An in-group mDNS sighting of `peer` advertising sync protocol `proto`: discovery skips a
    /// peer on another protocol (`txtodo_sync::PeerTable::observe`), so a LAN-only peer would
    /// never be dialed, and never named, without this. Our own protocol clears it.
    pub(crate) fn sighted_protocol(&self, peer: DeviceId, proto: u16) {
        if proto == txtodo_sync::PROTOCOL_VERSION {
            self.set_other_protocol(peer, None);
        } else if self.other_protocol(peer) != Some(proto) {
            self.book(Some(peer), PeerSignal::OtherProtocol(proto), "mdns");
        }
    }

    /// The sync protocol `peer` last spoke when it was not ours (module doc, "Another protocol").
    pub(crate) fn other_protocol(&self, peer: DeviceId) -> Option<u16> {
        self.lock()
            .get(&Some(peer))
            .and_then(|key| key.other_protocol)
    }

    fn set_other_protocol(&self, peer: DeviceId, protocol: Option<u16>) {
        let mut peers = self.lock();
        if let Some(key) = peers.get_mut(&Some(peer)) {
            key.other_protocol = protocol;
        } else if protocol.is_some() && peers.len() < MAX_TRACKED_PEERS {
            peers.entry(Some(peer)).or_default().other_protocol = protocol;
        }
    }

    /// Whether `peer` is parked: no dial loop should try it.
    pub(crate) fn is_parked(&self, peer: DeviceId) -> bool {
        self.lock().get(&Some(peer)).is_some_and(PeerKey::parked)
    }

    /// Ends `peer`'s `wrong_group` run, unparking it (`reason` is logged when it was parked).
    /// What was already warned about stays warned.
    pub(crate) fn forget(&self, peer: DeviceId, reason: &'static str) {
        let was_parked = self.lock().get_mut(&Some(peer)).is_some_and(|key| {
            let parked = key.parked();
            key.wrong_group = 0;
            parked
        });
        if was_parked {
            log_unparked(peer, reason);
        }
    }

    /// Our group changed: nothing seen under the old one says anything about the new one.
    pub(crate) fn clear(&self) {
        self.lock().clear();
        self.clear_epoch_misses();
    }

    /// Counts an open failure when it is `unknown_epoch`; logs once as the run reaches
    /// [`DROPPED_AFTER`].
    fn note_epoch_miss(&self, kind: &'static str) {
        if kind != UNKNOWN_EPOCH {
            return;
        }
        let run = self
            .epoch_misses
            .fetch_add(1, Ordering::Relaxed)
            .saturating_add(1);
        self.first_miss
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get_or_insert_with(Instant::now);
        if run == DROPPED_AFTER {
            log_dropped_from_group();
        }
    }

    /// Ends the run of `unknown_epoch` opens: a frame opened, or the epoch or group moved.
    pub(crate) fn clear_epoch_misses(&self) {
        self.epoch_misses.store(0, Ordering::Relaxed);
        *self
            .first_miss
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = None;
    }

    /// Whether every session lately showed only frames this device holds no key for: it was left
    /// out of the group and has to pair again.
    pub(crate) fn dropped_from_group(&self) -> bool {
        self.dropped_within(dropped_window())
    }

    /// [`PeerKeys::dropped_from_group`] with the window given: [`DROPPED_AFTER`] misses in a row, or
    /// any run that began `window` ago.
    pub(crate) fn dropped_within(&self, window: Duration) -> bool {
        let run = self.epoch_misses.load(Ordering::Relaxed);
        if run >= DROPPED_AFTER {
            return true;
        }
        let first = *self
            .first_miss
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        run > 0 && first.is_some_and(|at| at.elapsed() >= window)
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, BTreeMap<Option<DeviceId>, PeerKey>> {
        self.peers.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

fn log_dropped_from_group() {
    tracing::warn!(after = DROPPED_AFTER, "dropped_from_group");
}

fn peer_name(peer: Option<DeviceId>) -> String {
    peer.map_or_else(|| "unknown".to_owned(), |p| p.to_string())
}

/// Split in two only because each `tracing` macro counts heavily toward `cognitive_complexity`.
fn log_open_failed(peer: Option<DeviceId>, kind: &'static str, channel: &'static str, first: bool) {
    let peer = peer_name(peer);
    if first {
        log_open_failed_first(&peer, kind, channel);
    } else {
        tracing::debug!(%peer, kind, channel, "peer_open_failed");
    }
}

fn log_open_failed_first(peer: &str, kind: &'static str, channel: &'static str) {
    tracing::warn!(peer, kind, channel, "peer_open_failed");
}

fn log_parked(peer: Option<DeviceId>) {
    tracing::warn!(
        peer = %peer_name(peer),
        failures = PARK_AFTER,
        "peer_parked_no_shared_key"
    );
}

fn log_unparked(peer: DeviceId, reason: &'static str) {
    tracing::info!(%peer, reason, "peer_unparked");
}
