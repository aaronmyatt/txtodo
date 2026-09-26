//! Where sync from each peer is stuck (task sync-drift line 7): per peer and workspace, the last
//! file whose incoming run was refused, why, since when, and how many times in a row. The sender
//! resends an unacked run every `RESEND_AFTER`, so a run that can never land is refused again and
//! again, and every later op from that peer waits behind it. Before this only the log said so
//! (`lan_sync_ops_refused`, which names no peer). `SyncStatus` carries it, for `txtodo doctor`
//! and the TUI.
//!
//! A record clears once that peer's run for the same file lands; a run of ops we already hold
//! counts (`lan_apply.rs`). A refusal on another file replaces it. Device-wide and in memory only,
//! like `peer_keys.rs`: a restart or a group change forgets it, and the next refusal books it again.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, PoisonError};

use txtodo_model::{DeviceId, FilePath};
use txtodo_proto::v1 as pb;
use txtodo_store::WorkspaceId;

use crate::lan_apply::Landed;

/// Most records held at once, one per peer and workspace: far more than one device-set has. A new
/// one past it is not booked; its refusals are still logged.
pub(crate) const MAX_STUCK: usize = 256;

/// One peer and workspace where sync is stuck.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Stuck {
    /// The file whose run was refused, workspace-relative.
    pub(crate) file: FilePath,
    /// Why the latest refusal happened, the text `lan_apply.rs` logged.
    pub(crate) reason: String,
    /// The first refusal of this row, on the workspace clock.
    pub(crate) since_ms: u64,
    /// The latest refusal.
    pub(crate) last_ms: u64,
    /// Refusals of this file's run in a row, at least 1.
    pub(crate) refusals: u32,
}

type Key = (DeviceId, WorkspaceId);

/// Cheap to clone: one `Arc<Mutex<_>>`, like `PeerKeys`.
#[derive(Clone, Default)]
pub(crate) struct StuckSync {
    stuck: Arc<Mutex<BTreeMap<Key, Stuck>>>,
}

impl StuckSync {
    /// Books what one batch from `peer` into `workspace` did (`lan_apply::commit_incoming_ops`):
    /// a landed run for the stuck file clears the record, then a refused run books or bumps one.
    pub(crate) fn book(&self, peer: DeviceId, workspace: WorkspaceId, landed: &Landed, now: u64) {
        let key = (peer, workspace);
        let mut stuck = self.lock();
        if stuck
            .get(&key)
            .is_some_and(|s| landed.files.contains(&s.file))
        {
            stuck.remove(&key).inspect(|was| log_unstuck(key, was));
        }
        let Some((file, reason)) = &landed.refused else {
            return;
        };
        if let Some(s) = stuck.get_mut(&key).filter(|s| s.file == *file) {
            s.refusals = s.refusals.saturating_add(1);
            s.last_ms = now;
            s.reason.clone_from(reason);
            return;
        }
        if stuck.len() >= MAX_STUCK && !stuck.contains_key(&key) {
            return;
        }
        let row = Stuck {
            file: file.clone(),
            reason: reason.clone(),
            since_ms: now,
            last_ms: now,
            refusals: 1,
        };
        log_stuck(key, &row);
        stuck.insert(key, row);
    }

    /// Every workspace where sync from `peer` is stuck, in workspace order.
    pub(crate) fn of(&self, peer: DeviceId) -> Vec<(WorkspaceId, Stuck)> {
        self.lock()
            .iter()
            .filter(|((p, _), _)| *p == peer)
            .map(|((_, w), s)| (*w, s.clone()))
            .collect()
    }

    /// Our group changed: what peers of the old one could not send says nothing about the new.
    pub(crate) fn clear(&self) {
        self.lock().clear();
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, BTreeMap<Key, Stuck>> {
        self.stuck.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// One record on the wire (`SyncStatus`, `devices_grpc.rs`).
pub(crate) fn to_pb(workspace: WorkspaceId, s: Stuck) -> pb::sync_status_response::Stuck {
    pb::sync_status_response::Stuck {
        workspace_id: workspace.to_string(),
        file: s.file.as_str().to_owned(),
        reason: s.reason,
        since_ms: s.since_ms,
        last_ms: s.last_ms,
        refusals: s.refusals,
    }
}

/// Warn once per row, with the peer: `lan_sync_ops_refused` repeats on every resend and names none.
fn log_stuck((peer, workspace): Key, s: &Stuck) {
    tracing::warn!(%peer, %workspace, file = %s.file, reason = %s.reason, "lan_sync_stuck");
}

fn log_unstuck((peer, workspace): Key, was: &Stuck) {
    tracing::info!(
        %peer,
        %workspace,
        file = %was.file,
        refusals = was.refusals,
        "lan_sync_unstuck"
    );
}

#[cfg(test)]
#[path = "stuck_sync_tests.rs"]
mod tests;
