//! The actor's mirror upkeep (plan M4): flushing committed ops, converging after an adopt,
//! rebuilding as the last resort, raising flags and the commit extras. Same `impl FileActor`;
//! split from `actor.rs` for the file budget. None of these can fail a client's change — the
//! change is durable before any of them runs; problems are logged and healed.

use crate::actor::{CommitTail, FileActor};
use crate::handle::ActorError;
use crate::identity_fingerprint::fingerprint_of;
use crate::mirror::{Mirror, MirrorError};
use crate::reconcile::task_of;
use crate::state::DocState;
use txtodo_model::{DeviceId, Field, FilePath, IdentityMode, Op, OpKind};
use txtodo_store::{CommitExtras, FingerprintRow, ReviewRow};

/// Whether any op in the batch adds, removes or reorders list entries — the only kind of batch
/// `after_flush` pays a full `agrees_with` walk for. An empty batch never does.
fn reshapes_list(ops: &[Op]) -> bool {
    ops.iter().any(|o| {
        matches!(
            o.kind,
            OpKind::Insert { .. }
                | OpKind::Move { .. }
                | OpKind::BlankInsert { .. }
                | OpKind::BlankRemove { .. }
                // A tombstone drops the task out of the visible list.
                | OpKind::SetField {
                    field: Field::Deleted,
                    ..
                }
        )
    })
}

/// The Loro peer id for a device: the ULID's low 64 bits (its random half).
pub(crate) fn loro_peer(device: DeviceId) -> u64 {
    let bits = device.ulid().to_u128();
    let peer = (bits & u128::from(u64::MAX)) as u64;
    debug_assert_eq!(u128::from(peer), bits & u128::from(u64::MAX));
    peer
}

impl FileActor {
    /// The Loro updates a peer at `since` is missing.
    pub(crate) fn on_export(&mut self, since: &[u8]) -> Result<Vec<u8>, ActorError> {
        // `after_flush` walks the whole list only for batches that reshape it, so a field or text
        // edit can leave the mirror disagreeing until something looks. This is the export-time
        // look: heal first (a converge keeps the lineage, a rebuild is its last resort), so one
        // bad text edit does not block this document's sync until an unrelated insert or move
        // (root todo id:01M2WK5DQQF0XXZXJP18JDWYDT). One walk per export, off the commit hot path.
        if !self.mirror.agrees_with(&self.state) {
            self.log_export_disagreed();
            self.converge_mirror();
        }
        // A mirror every heal failed on (`resync_mirror`'s last resort included) must never
        // reach a peer: refuse the export rather than ship a known-wrong document.
        if !self.mirror.agrees_with(&self.state) {
            return Err(ActorError::Mirror(
                "mirror disagrees with the state; export refused".to_owned(),
            ));
        }
        let bytes = self
            .mirror
            .export_since(since)
            .map_err(|e| ActorError::Mirror(e.to_string()))?;
        debug_assert!(!bytes.is_empty());
        Ok(bytes)
    }

    fn log_export_disagreed(&self) {
        tracing::error!(file = %self.cfg.path, "mirror_export_disagreed_converging");
    }

    /// The store-transaction extras a commit tail asks for.
    pub(crate) fn commit_extras(
        &self,
        tail: &CommitTail,
        next: &DocState,
    ) -> Result<CommitExtras, ActorError> {
        let mirror = if tail.persist_mirror {
            Some(
                self.mirror
                    .snapshot()
                    .map_err(|e| ActorError::Mirror(e.to_string()))?,
            )
        } else {
            None
        };
        debug_assert_eq!(mirror.is_some(), tail.persist_mirror);
        debug_assert!(mirror.as_ref().is_none_or(|m| !m.is_empty()));
        Ok(CommitExtras {
            clear: tail.clear,
            mirror,
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

    /// Feeds committed ops to the mirror; a refusal is a bug in the mirror, logged and healed by
    /// converging from the state — never surfaced to the client, whose change is already durable.
    pub(crate) fn flush_mirror(&mut self, ops: &[Op]) {
        let result = self.mirror.flush(ops, &self.state);
        self.after_flush(ops, result);
    }

    fn after_flush(&mut self, ops: &[Op], result: Result<(), MirrorError>) {
        match result {
            // The full-document walk only runs for a batch that can change the list's shape:
            // field and text edits are settled per task by `flush` itself, and paying the walk
            // on every such commit puts O(document) on the hot path.
            Ok(()) if !reshapes_list(ops) || self.mirror.agrees_with(&self.state) => {
                self.log_flushed(ops.len());
            }
            // A flush that returned Ok can still leave the mirror disagreeing with the state
            // (task `daemon-mirror-assertion-panic`: this used to be a debug-only assertion, a
            // real panic in dev/test builds and a silently wrong mirror in release — neither
            // ever reached a client, since the mirror is never consulted for bytes, but a wrong
            // mirror is a real bug in what sync exports). Escalate exactly like a refusal: never
            // trust an unchecked "it said Ok" past this point.
            Ok(()) => {
                self.log_flush_disagreed();
                self.converge_mirror();
            }
            Err(e) => self.flush_refused(&e),
        }
    }

    fn log_flushed(&self, ops: usize) {
        tracing::debug!(file = %self.cfg.path, ops, "mirror_flushed");
    }

    fn log_flush_disagreed(&self) {
        tracing::error!(file = %self.cfg.path, "mirror_flush_disagreed_converging");
    }

    fn flush_refused(&mut self, e: &MirrorError) {
        tracing::error!(file = %self.cfg.path, error = %e, "mirror_refused_converging");
        self.converge_mirror();
    }

    /// Brings the mirror to the state with corrective ops, keeping its lineage; only if that
    /// fails too — or still disagrees afterward — is it rebuilt from scratch (a new lineage,
    /// logged as such).
    pub(crate) fn converge_mirror(&mut self) {
        let result = self.mirror.converge_to(&self.state, self.hlc);
        self.after_converge(result);
    }

    fn after_converge(&mut self, result: Result<usize, MirrorError>) {
        match result {
            Ok(n) if self.mirror.agrees_with(&self.state) => self.log_converged(n),
            // Same reasoning as `after_flush`: a converge that reported success but still
            // disagrees is exactly the bug this task fixed the known cause of (a walk-local
            // placeholder collision in `mirror_converge.rs::place`) — checked here, always, not
            // only in a debug build, in case a different divergence is ever introduced later.
            Ok(n) => {
                self.log_converge_disagreed(n);
                self.resync_mirror();
            }
            Err(e) => self.converge_failed(&e),
        }
    }

    fn log_converged(&self, ops: usize) {
        tracing::info!(file = %self.cfg.path, ops, "mirror_converged");
    }

    fn log_converge_disagreed(&self, ops: usize) {
        tracing::error!(file = %self.cfg.path, ops, "mirror_converge_disagreed_new_lineage");
    }

    fn converge_failed(&mut self, e: &MirrorError) {
        tracing::error!(file = %self.cfg.path, error = %e, "mirror_converge_failed_new_lineage");
        self.resync_mirror();
    }

    /// Rebuilds the mirror from the state: a new Loro lineage. For a first open with no
    /// snapshot, or as the last resort — nothing left to escalate to, so a lingering
    /// disagreement here can only be logged, not healed further.
    pub(crate) fn resync_mirror(&mut self) {
        match Mirror::from_state(&self.state, loro_peer(self.cfg.device)) {
            Ok(m) => self.mirror = m,
            Err(e) => Self::log_rebuild_failed(&self.cfg.path, &e),
        }
        if !self.mirror.agrees_with(&self.state) {
            tracing::error!(file = %self.cfg.path, "mirror_resync_still_disagrees");
        }
    }

    fn log_rebuild_failed(path: &FilePath, e: &MirrorError) {
        tracing::error!(file = %path, error = %e, "mirror_rebuild_failed");
    }

    /// Stores the flags a change raised; a store failure is logged, the change is already durable.
    pub(crate) fn raise_flags(&mut self, rows: &[ReviewRow]) {
        if rows.is_empty() {
            return;
        }
        let path = self.cfg.path.clone();
        let mut store = self.lock_store();
        for row in rows {
            if let Err(e) = store.raise_flag(row) {
                Self::log_raise_flag_failed(&path, &e);
            }
        }
        debug_assert!(rows.iter().all(|r| r.file == self.cfg.path));
    }

    fn log_raise_flag_failed(path: &FilePath, e: &txtodo_store::StoreError) {
        tracing::error!(file = %path, error = %e, "raise_flag_failed");
    }

    /// Sends `change` to every `Watch` subscriber; a full mailbox never blocks the commit that
    /// just landed durably (`Err` only means no receiver is left, which the guard excludes).
    pub(crate) fn broadcast(&self, change: &crate::handle::Change) {
        if self.changes.receiver_count() > 0 {
            let _ = self.changes.send(change.clone());
        }
    }
}
