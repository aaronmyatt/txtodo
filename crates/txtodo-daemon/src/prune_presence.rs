//! When this device last came back from being off or asleep (task `peer-prune`, ADR 0044): the
//! stale verdict (`prune_stale.rs`) counts a peer's silence only from here, so a laptop that was shut
//! for a week does not wake up and drop every peer it could not hear. A prune tick that comes more
//! than [`AWAY_GAP_MS`] after the last one (a sleep, a restart after a stop) moves it to now. Kept in
//! `meta` so a restart sees the gap too.

use txtodo_store::IdentityStore;

use crate::prune_stale::PruneConfig;

/// A gap between two ticks longer than this means the device was away. The tick is seconds apart.
pub(crate) const AWAY_GAP_MS: u64 = 10 * 60_000;

/// The gap that counts as away under `cfg`: [`AWAY_GAP_MS`], or half the shortest threshold when
/// that is shorter, so a device paused for longer than that never judges its peers on silence from
/// while it was paused. The defaults (a day, a week) leave it at ten minutes.
pub(crate) fn away_gap_ms(cfg: &PruneConfig) -> u64 {
    [cfg.parked_after_ms, cfg.quiet_after_ms]
        .into_iter()
        .filter(|ms| *ms > 0)
        .map(|ms| ms / 2)
        .fold(AWAY_GAP_MS, u64::min)
}

/// The tick time is written at most this often: a restart is judged against it, to within a minute.
const SAVE_EVERY_MS: u64 = 60_000;

const LAST_TICK_KEY: &str = "prune_last_tick_ms";
const UP_SINCE_KEY: &str = "prune_up_since_ms";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Presence {
    last_tick_ms: u64,
    up_since_ms: u64,
    saved_tick_ms: u64,
}

impl Presence {
    /// Reads both times back; a fresh device has neither, so its first tick counts as coming back.
    pub(crate) fn load(store: &IdentityStore) -> Presence {
        let read = |key: &str| {
            let bytes = store.meta_get(key).ok().flatten()?;
            Some(u64::from_be_bytes(bytes.try_into().ok()?))
        };
        let last_tick_ms = read(LAST_TICK_KEY).unwrap_or(0);
        Presence {
            last_tick_ms,
            up_since_ms: read(UP_SINCE_KEY).unwrap_or(0),
            saved_tick_ms: last_tick_ms,
        }
    }

    /// One tick at `now_ms`: the time this device came back, which is now after an away gap. What
    /// has to be written (the new `up_since`, or a tick time not saved for a minute) goes to `store`.
    pub(crate) fn tick(&mut self, store: &mut IdentityStore, now_ms: u64, away_gap_ms: u64) -> u64 {
        let away = now_ms.saturating_sub(self.last_tick_ms) > away_gap_ms;
        if away {
            self.up_since_ms = now_ms;
            save(store, UP_SINCE_KEY, now_ms);
        }
        self.last_tick_ms = now_ms;
        if away || now_ms.saturating_sub(self.saved_tick_ms) >= SAVE_EVERY_MS {
            self.saved_tick_ms = now_ms;
            save(store, LAST_TICK_KEY, now_ms);
        }
        self.up_since_ms
    }
}

/// Best effort: a failed write only means a restart may judge the gap a little wrong.
fn save(store: &mut IdentityStore, key: &str, ms: u64) {
    if let Err(e) = store.meta_set(key, &ms.to_be_bytes()) {
        log_save_failed(key, &e);
    }
}

fn log_save_failed(key: &str, error: &txtodo_store::StoreError) {
    tracing::debug!(key, %error, "prune_presence_save_failed");
}

#[cfg(test)]
#[path = "prune_presence_tests.rs"]
mod tests;
