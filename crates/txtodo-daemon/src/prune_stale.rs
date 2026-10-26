//! Which peers are stale (task `peer-prune`, ADR 0044 decision 2): a pure verdict from this device's
//! own `devices` row and its `peer_keys` parked flag, so each device decides alone. A peer is stale
//! when it is **parked** (three `wrong_group` opens in a row, `peer_keys.rs`) and silent for
//! `parked_after`, or merely **quiet** (keys match, not heard from) for `quiet_after`. `0` turns a
//! rule off. The verdict never prunes; `prune_loop.rs` acts on it.
//!
//! "Silent" is counted from the newest of the peer's `last_seen`, its `paired_at` and `up_since`, the
//! last moment this device came back from being off or asleep: a laptop shut for a week must not wake
//! up and drop every peer it could not hear while it was away.

use txtodo_model::DeviceId;
use txtodo_store::DeviceRow;

use crate::device_identity::DeviceIdentity;

/// One day, in milliseconds.
pub(crate) const DAY_MS: u64 = 86_400_000;

/// A parked peer is stale after a day (agent pick, ADR 0044): its keys do not match ours, so nothing
/// it sends can open, and waiting longer buys nothing.
pub(crate) const DEFAULT_PARKED_AFTER_MS: u64 = DAY_MS;

/// A peer whose keys match but that is not heard from is stale after a week (agent pick, ADR 0044): a
/// device left in a drawer for a few days must not be dropped.
pub(crate) const DEFAULT_QUIET_AFTER_MS: u64 = 7 * DAY_MS;

/// How long a peer may stay silent, per rule. `0` is that rule off.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PruneConfig {
    pub(crate) parked_after_ms: u64,
    pub(crate) quiet_after_ms: u64,
}

impl Default for PruneConfig {
    fn default() -> Self {
        PruneConfig {
            parked_after_ms: DEFAULT_PARKED_AFTER_MS,
            quiet_after_ms: DEFAULT_QUIET_AFTER_MS,
        }
    }
}

/// Why a peer is stale.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Stale {
    /// Its frames do not open under our group key, and it has been silent for `parked_after`.
    Parked,
    /// Its keys match and it has been silent for `quiet_after`.
    Quiet,
}

impl Stale {
    /// The word logged and shown for this reason.
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Stale::Parked => "parked",
            Stale::Quiet => "quiet",
        }
    }
}

/// `after` milliseconds of silence, with `0` meaning the rule is off.
fn due(after_ms: u64, silent_ms: u64) -> bool {
    after_ms > 0 && silent_ms >= after_ms
}

/// The verdict for one peer's row; `None` for a removed one (nothing left to prune).
pub(crate) fn verdict(
    row: &DeviceRow,
    parked: bool,
    cfg: &PruneConfig,
    now_ms: u64,
    up_since_ms: u64,
) -> Option<Stale> {
    if row.removed_at_ms.is_some() {
        return None;
    }
    let heard = row
        .last_seen_ms
        .unwrap_or(0)
        .max(row.paired_at_ms)
        .max(up_since_ms);
    let silent = now_ms.saturating_sub(heard);
    if parked && due(cfg.parked_after_ms, silent) {
        Some(Stale::Parked)
    } else if due(cfg.quiet_after_ms, silent) {
        Some(Stale::Quiet)
    } else {
        None
    }
}

/// Every stale peer of `identity`, in device order. This device is never in its own table.
pub(crate) fn stale_peers(
    identity: &DeviceIdentity,
    cfg: &PruneConfig,
    now_ms: u64,
    up_since_ms: u64,
) -> Vec<(DeviceId, Stale)> {
    let rows = identity
        .store()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .list_devices()
        .unwrap_or_default();
    rows.iter()
        .filter_map(|row| {
            let parked = identity.peer_keys().is_parked(row.device);
            Some((row.device, verdict(row, parked, cfg, now_ms, up_since_ms)?))
        })
        .collect()
}

/// A duration as `prune_parked_after`/`prune_quiet_after` write it: digits and a unit, `s`, `m`,
/// `h` or `d`; bare digits are seconds. `None` for anything else (empty, a sign, a fraction).
pub(crate) fn parse_duration_ms(raw: &str) -> Option<u64> {
    let raw = raw.trim();
    // `char_indices` + `find` split the digits from the unit without a regex.
    // Ref: https://doc.rust-lang.org/std/primitive.str.html#method.char_indices
    let (digits, unit) = match raw.char_indices().find(|(_, c)| !c.is_ascii_digit()) {
        Some((at, _)) => raw.split_at(at),
        None => (raw, "s"),
    };
    let n: u64 = digits.parse().ok()?;
    let unit_ms: u64 = match unit.trim() {
        "s" => 1_000,
        "m" => 60_000,
        "h" => 3_600_000,
        "d" => DAY_MS,
        _ => return None,
    };
    n.checked_mul(unit_ms)
}

#[cfg(test)]
#[path = "prune_stale_tests.rs"]
mod tests;
