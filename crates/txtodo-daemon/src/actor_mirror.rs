//! The actor's mirror upkeep (plan M4): flushing committed ops, converging after an adopt,
//! rebuilding as the last resort, raising flags and the commit extras. Same `impl FileActor`;
//! split from `actor.rs` for the file budget. None of these can fail a client's change — the
//! change is durable before any of them runs; problems are logged and healed.

use crate::actor::{CommitTail, FileActor};
use crate::handle::ActorError;
use crate::mirror::{Mirror, MirrorError};
use txtodo_model::{DeviceId, FilePath, Op};
use txtodo_store::{CommitExtras, ReviewRow};

/// The Loro peer id for a device: the ULID's low 64 bits (its random half).
pub(crate) fn loro_peer(device: DeviceId) -> u64 {
    let bits = device.ulid().to_u128();
    let peer = (bits & u128::from(u64::MAX)) as u64;
    debug_assert_eq!(u128::from(peer), bits & u128::from(u64::MAX));
    peer
}

impl FileActor {
    /// The Loro updates a peer at `since` is missing.
    pub(crate) fn on_export(&self, since: &[u8]) -> Result<Vec<u8>, ActorError> {
        let bytes = self
            .mirror
            .export_since(since)
            .map_err(|e| ActorError::Mirror(e.to_string()))?;
        debug_assert!(!bytes.is_empty());
        Ok(bytes)
    }

    /// The store-transaction extras a commit tail asks for.
    pub(crate) fn commit_extras(&self, tail: &CommitTail) -> Result<CommitExtras, ActorError> {
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
        })
    }

    /// Feeds committed ops to the mirror; a refusal is a bug in the mirror, logged and healed by
    /// converging from the state — never surfaced to the client, whose change is already durable.
    pub(crate) fn flush_mirror(&mut self, ops: &[Op]) {
        match self.mirror.flush(ops, &self.state) {
            Ok(()) => tracing::debug!(file = %self.cfg.path, ops = ops.len(), "mirror_flushed"),
            Err(e) => self.flush_refused(&e),
        }
        debug_assert!(ops.is_empty() || self.mirror.agrees_with(&self.state));
    }

    fn flush_refused(&mut self, e: &MirrorError) {
        tracing::error!(file = %self.cfg.path, error = %e, "mirror_refused_converging");
        self.converge_mirror();
    }

    /// Brings the mirror to the state with corrective ops, keeping its lineage; only if that
    /// fails too is it rebuilt from scratch (a new lineage, logged as such).
    pub(crate) fn converge_mirror(&mut self) {
        match self.mirror.converge_to(&self.state, self.hlc) {
            Ok(n) => tracing::info!(file = %self.cfg.path, ops = n, "mirror_converged"),
            Err(e) => self.converge_failed(&e),
        }
        debug_assert!(self.mirror.agrees_with(&self.state));
    }

    fn converge_failed(&mut self, e: &MirrorError) {
        tracing::error!(file = %self.cfg.path, error = %e, "mirror_converge_failed_new_lineage");
        self.resync_mirror();
    }

    /// Rebuilds the mirror from the state: a new Loro lineage. For a first open with no
    /// snapshot, or as the last resort.
    pub(crate) fn resync_mirror(&mut self) {
        match Mirror::from_state(&self.state, loro_peer(self.cfg.device)) {
            Ok(m) => self.mirror = m,
            Err(e) => Self::log_rebuild_failed(&self.cfg.path, &e),
        }
        debug_assert!(self.mirror.agrees_with(&self.state));
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
}
