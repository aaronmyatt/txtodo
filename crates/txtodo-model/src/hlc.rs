//! Hybrid logical clock. Ref: Kulkarni et al., "Logical Physical Clocks and Consistent Snapshots"
//! <https://cse.buffalo.edu/tech-reports/2014-04.pdf> §3. Total order `(wall_ms, counter, device)`;
//! `tick` (send rule) never goes backwards even when the wall clock does; `merge` (receive rule)
//! folds a peer's stamp in. The skew guard is asymmetric on purpose: a peer *ahead* would ratchet
//! every clock that ever syncs with us forward for good, so it is refused; a peer *behind* only
//! sorts its own ops early, so it merges and the caller may warn (plan M4).

use crate::DeviceId;
use serde::{Deserialize, Serialize};

/// Largest peer clock lead we will merge. Beyond this the peer's clock, not ours, is wrong.
pub const MAX_PEER_SKEW_AHEAD_MS: u64 = 5 * 60 * 1_000;
/// Lag past which the caller should warn the human; merging is still safe.
pub const MAX_PEER_SKEW_BEHIND_MS: u64 = 5 * 60 * 1_000;

/// One HLC stamp. Derived `Ord` is field order, which is the HLC order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Hlc {
    /// Physical component, Unix milliseconds.
    pub wall_ms: u64,
    /// Logical component; breaks ties within one millisecond.
    pub counter: u16,
    /// Tie-breaker across devices.
    pub device: DeviceId,
}

/// Why a stamp could not be produced. Closed set; every `match` is exhaustive.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HlcError {
    /// More than `u16::MAX` stamps in one millisecond.
    Overflow {
        /// The wall time that overflowed.
        wall_ms: u64,
    },
    /// The peer's wall clock leads ours by more than the bound; its stamp was not merged.
    PeerAhead {
        /// The peer stamp's wall time.
        peer_ms: u64,
        /// Our wall time at the merge.
        local_ms: u64,
        /// The bound that was exceeded (`MAX_PEER_SKEW_AHEAD_MS`).
        bound_ms: u64,
    },
}

impl core::fmt::Display for HlcError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            HlcError::Overflow { wall_ms } => {
                write!(f, "hlc counter overflow at wall_ms {wall_ms}")
            }
            HlcError::PeerAhead {
                peer_ms,
                local_ms,
                bound_ms,
            } => write!(
                f,
                "refused to merge peer clock {peer_ms} ms: it leads local clock {local_ms} ms by \
                 {} ms, more than the {bound_ms} ms bound",
                peer_ms.saturating_sub(*local_ms)
            ),
        }
    }
}

impl std::error::Error for HlcError {}

/// How a peer's wall clock compares to ours, against the two bounds. Shared by `Hlc::merge`, the
/// sync `Hello` handshake and `txtodo doctor` so there is one rule.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Skew {
    /// Within both bounds.
    Ok,
    /// The peer lags by this many ms, more than `MAX_PEER_SKEW_BEHIND_MS`. Safe; warn.
    Behind(u64),
    /// The peer leads by this many ms, more than `MAX_PEER_SKEW_AHEAD_MS`. Refuse.
    Ahead(u64),
}

impl Skew {
    /// Classifies `peer_ms` against `local_ms`. Thin wrapper around `check_inner` for the tracing
    /// span (`#[instrument]` on top of the existing three-way branch risks the
    /// `cognitive_complexity` budget). Shared by `Hlc::merge`, the sync `Hello` handshake and
    /// `txtodo doctor` (module doc above), so this one site's logging covers all three callers.
    #[tracing::instrument(skip_all)]
    pub fn check(peer_ms: u64, local_ms: u64) -> Skew {
        let skew = Skew::check_inner(peer_ms, local_ms);
        log_skew_checked(peer_ms, local_ms, skew);
        skew
    }

    fn check_inner(peer_ms: u64, local_ms: u64) -> Skew {
        let lead = peer_ms.saturating_sub(local_ms);
        let lag = local_ms.saturating_sub(peer_ms);
        debug_assert!(
            lead == 0 || lag == 0,
            "a clock is not both ahead and behind"
        );
        let skew = if lead > MAX_PEER_SKEW_AHEAD_MS {
            Skew::Ahead(lead)
        } else if lag > MAX_PEER_SKEW_BEHIND_MS {
            Skew::Behind(lag)
        } else {
            Skew::Ok
        };
        debug_assert!(
            skew != Skew::Ok || (lead <= MAX_PEER_SKEW_AHEAD_MS && lag <= MAX_PEER_SKEW_BEHIND_MS),
            "Ok means within both bounds"
        );
        skew
    }
}

/// Logs the skew-guard decision: the peer/local wall-clock ms, the classification, and the lead
/// or lag amount when not `Ok`. This is the one place a peer's clock is judged against ours, so it
/// is the site root todo.txt calls out as deciding silently today. Never logs anything beyond the
/// clock fields themselves — no task, op, or file identity ever reaches `Skew::check`.
fn log_skew_checked(peer_ms: u64, local_ms: u64, skew: Skew) {
    let (label, lead_ms, lag_ms) = match skew {
        Skew::Ok => ("ok", None, None),
        Skew::Ahead(lead) => ("ahead", Some(lead), None),
        Skew::Behind(lag) => ("behind", None, Some(lag)),
    };
    tracing::debug!(
        peer_ms,
        local_ms,
        skew = label,
        lead_ms,
        lag_ms,
        "hlc_skew_checked"
    );
}

impl Hlc {
    /// The zero stamp for a device; every `tick` is greater than this.
    pub const fn zero(device: DeviceId) -> Hlc {
        Hlc {
            wall_ms: 0,
            counter: 0,
            device,
        }
    }

    /// Send rule: advances to a stamp strictly greater than `self`, using `now_ms` when it is
    /// ahead. Thin wrapper around `tick_inner` for the tracing span (`#[instrument]` on top of the
    /// existing branch risks the `cognitive_complexity` budget).
    #[tracing::instrument(skip_all)]
    pub fn tick(&mut self, now_ms: u64) -> Result<Hlc, HlcError> {
        let result = self.tick_inner(now_ms);
        log_tick(now_ms, &result);
        result
    }

    fn tick_inner(&mut self, now_ms: u64) -> Result<Hlc, HlcError> {
        let before = *self;
        if now_ms > self.wall_ms {
            self.wall_ms = now_ms;
            self.counter = 0;
        } else {
            self.counter = self.counter.checked_add(1).ok_or(HlcError::Overflow {
                wall_ms: self.wall_ms,
            })?;
        }
        debug_assert!(*self > before, "tick is strictly monotone");
        debug_assert_eq!(self.device, before.device, "tick never changes the device");
        Ok(*self)
    }

    /// Receive rule (Kulkarni §3): folds `remote` in and returns a stamp greater than both `self`
    /// and `remote`. A peer more than `MAX_PEER_SKEW_AHEAD_MS` ahead of `now_ms` is refused with
    /// `PeerAhead`; on any `Err` the clock is unchanged. Thin wrapper around `merge_inner` for the
    /// tracing span (`#[instrument]` on top of the existing skew-refusal branch and four-way
    /// counter-selection `match` risks the `cognitive_complexity` budget).
    #[tracing::instrument(skip_all)]
    pub fn merge(&mut self, remote: Hlc, now_ms: u64) -> Result<Hlc, HlcError> {
        let result = self.merge_inner(remote, now_ms);
        log_merge(&remote, now_ms, &result);
        result
    }

    fn merge_inner(&mut self, remote: Hlc, now_ms: u64) -> Result<Hlc, HlcError> {
        if let Skew::Ahead(_) = Skew::check(remote.wall_ms, now_ms) {
            return Err(HlcError::PeerAhead {
                peer_ms: remote.wall_ms,
                local_ms: now_ms,
                bound_ms: MAX_PEER_SKEW_AHEAD_MS,
            });
        }
        let before = *self;
        let wall_ms = self.wall_ms.max(remote.wall_ms).max(now_ms);
        let counter = match (wall_ms == self.wall_ms, wall_ms == remote.wall_ms) {
            (true, true) => self.counter.max(remote.counter).checked_add(1),
            (true, false) => self.counter.checked_add(1),
            (false, true) => remote.counter.checked_add(1),
            (false, false) => Some(0),
        }
        .ok_or(HlcError::Overflow { wall_ms })?;
        let next = Hlc {
            wall_ms,
            counter,
            device: before.device,
        };
        debug_assert!(next > before, "merge is strictly monotone");
        debug_assert!(next > remote, "merge dominates the remote stamp");
        debug_assert_eq!(next.device, before.device, "merge never changes the device");
        *self = next;
        Ok(next)
    }
}

/// Logs the send-rule outcome: `now_ms`, the resulting `wall_ms`/`counter` (`None` on `Overflow`),
/// and whether the tick failed. Never logs the device beyond the caller's own span.
fn log_tick(now_ms: u64, result: &Result<Hlc, HlcError>) {
    let stamp = result.as_ref().ok().copied();
    tracing::debug!(
        now_ms,
        wall_ms = stamp.map(|s| s.wall_ms),
        counter = stamp.map(|s| s.counter),
        overflow = result.is_err(),
        "hlc_tick"
    );
}

/// Logs the receive-rule outcome: the remote stamp's wall-clock ms and device, `now_ms`, the
/// resulting `wall_ms`/`counter` (`None` on any `Err`), and whether the merge was refused. Does
/// not re-decide or re-log the skew classification — `Skew::check`'s own event already fires
/// inside `merge_inner` and covers whether the peer was within tolerance.
fn log_merge(remote: &Hlc, now_ms: u64, result: &Result<Hlc, HlcError>) {
    let stamp = result.as_ref().ok().copied();
    tracing::debug!(
        remote_wall_ms = remote.wall_ms,
        remote_device = %remote.device,
        now_ms,
        wall_ms = stamp.map(|s| s.wall_ms),
        counter = stamp.map(|s| s.counter),
        refused = result.is_err(),
        "hlc_merge"
    );
}
