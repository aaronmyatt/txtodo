//! The prune loop (task `peer-prune`, ADR 0044): once per resync tick each device judges its own
//! peers (`prune_stale.rs`) and runs the same `remove_device` that `txtodo device remove` does on
//! every stale one, so the group key rotates and the grant goes out in the control channel
//! (`control_flow.rs`). Every device decides alone from its own `last_seen`; a receiver follows a
//! grant, so one device's prune holds for the group (`group_rotation.rs`). The thresholds come from
//! `prune_config.rs`, and silence is counted from when this device last came back (`prune_presence.rs`).

use std::collections::BTreeSet;
use std::sync::{Arc, PoisonError};

use tokio::task::JoinHandle;
use txtodo_model::DeviceId;

use crate::clock::Clock;
use crate::device_identity::DeviceIdentity;
use crate::prune_presence::Presence;
use crate::prune_stale::{PruneConfig, Stale, stale_peers};

/// The background prune task. Dropping it stops the task.
pub struct PruneTransport {
    task: JoinHandle<()>,
}

impl Drop for PruneTransport {
    fn drop(&mut self) {
        self.task.abort();
    }
}

/// Starts the loop with the thresholds of this process (`prune_config::load`). Both rules off
/// (`0`) starts nothing.
pub fn start(identity: Arc<DeviceIdentity>, clock: Arc<dyn Clock>) -> Option<PruneTransport> {
    let cfg = crate::prune_config::load();
    if cfg.parked_after_ms == 0 && cfg.quiet_after_ms == 0 {
        log_pruning_off();
        return None;
    }
    let task = tokio::spawn(run(identity, clock, cfg));
    Some(PruneTransport { task })
}

async fn run(identity: Arc<DeviceIdentity>, clock: Arc<dyn Clock>, cfg: PruneConfig) {
    let mut presence = {
        let store = identity
            .store()
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        Presence::load(&store)
    };
    let mut failed = BTreeSet::new();
    // The first tick fires at once; `Presence::tick` has already moved `up_since` to now when this
    // device was away, so no peer is judged on silence from before that.
    // Ref: https://docs.rs/tokio/latest/tokio/time/struct.Interval.html
    let mut ticker = tokio::time::interval(crate::lan::resync_interval());
    loop {
        ticker.tick().await;
        let now_ms = clock.now_ms();
        let up_since_ms = {
            let mut store = identity
                .store()
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            presence.tick(&mut store, now_ms)
        };
        prune_once(&identity, &cfg, (now_ms, up_since_ms), &mut failed);
    }
}

/// One round: every stale peer is removed. `failed` holds the peers a removal already failed for,
/// so a persistent failure is warned about once, not every tick.
pub(crate) fn prune_once(
    identity: &DeviceIdentity,
    cfg: &PruneConfig,
    (now_ms, up_since_ms): (u64, u64),
    failed: &mut BTreeSet<DeviceId>,
) {
    for (peer, why) in stale_peers(identity, cfg, now_ms, up_since_ms) {
        match identity.remove_device(peer, now_ms) {
            Ok(outcome) => log_peer_pruned(peer, why, outcome.rotated_to_epoch),
            Err(e) if failed.insert(peer) => log_prune_failed(peer, why, &e),
            Err(_) => {}
        }
    }
}

fn log_peer_pruned(peer: DeviceId, why: Stale, epoch: u32) {
    tracing::info!(%peer, why = why.as_str(), epoch, "peer_pruned");
}

fn log_prune_failed(peer: DeviceId, why: Stale, error: &crate::device_remove::RemoveDeviceError) {
    tracing::warn!(%peer, why = why.as_str(), %error, "peer_prune_failed");
}

fn log_pruning_off() {
    tracing::info!("peer_pruning_off");
}

#[cfg(test)]
#[path = "prune_loop_tests.rs"]
mod tests;
