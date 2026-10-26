//! The actor's commit extras: fingerprints and the change broadcast. Same `impl FileActor`; split
//! from `actor.rs` for the file budget. The todo.txt Loro mirror this file used to keep in step,
//! and the review flags it raised, are gone (ADR 0038). Neither can fail a client's change: the
//! change is durable before either runs.

use crate::actor::{CommitTail, FileActor};
use crate::handle::ActorError;
use crate::identity_fingerprint::fingerprint_of;
use crate::reconcile::task_of;
use crate::state::DocState;
use txtodo_model::IdentityMode;
use txtodo_store::{CommitExtras, FingerprintRow};

impl FileActor {
    /// The store-transaction extras a commit tail asks for.
    pub(crate) fn commit_extras(
        &self,
        tail: &CommitTail,
        next: &DocState,
    ) -> Result<CommitExtras, ActorError> {
        Ok(CommitExtras {
            fingerprints: self.fingerprints_for(next),
            source: tail.source.clone(),
        })
    }

    /// Every live task's fingerprint in `next`, for `CommitExtras::fingerprints` — empty outside
    /// sidecar mode, where there is nothing to track (identity lives in the `id:` tag instead).
    fn fingerprints_for(&self, next: &DocState) -> Vec<FingerprintRow> {
        if self.cfg.identity_mode != IdentityMode::Sidecar {
            return Vec::new();
        }
        let now_ms = self.clock.now_ms();
        next.task_lines()
            .enumerate()
            .filter_map(|(i, (task, line))| {
                let parsed = task_of(line)?;
                Some(FingerprintRow {
                    file: self.cfg.path.clone(),
                    task,
                    fingerprint: fingerprint_of(&parsed, i),
                    updated_at_ms: now_ms,
                })
            })
            .collect()
    }

    /// Sends `change` to every `Watch` subscriber; a full mailbox never blocks the commit that
    /// just landed durably (`Err` only means no receiver is left, which the guard excludes).
    pub(crate) fn broadcast(&self, change: &crate::handle::Change) {
        self.cfg.stats.count_commit();
        // Another device's ops may be worth a notice (ADR 0041); the hub keeps only `sync`.
        if let Some(hub) = crate::notices::hub() {
            hub.take(change);
        }
        if self.changes.receiver_count() > 0 {
            let _ = self.changes.send(change.clone());
        }
    }
}
