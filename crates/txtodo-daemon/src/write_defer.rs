//! A peer batch's file write, deferred to the file's last run in the batch (task first-sync-speed,
//! option B, decided 2026-10-03). A received batch is committed one same-file run at a time, and
//! each commit used to rename the whole file with an fsync: ~40% of a sync commit. Now a run whose
//! file comes up again later in the batch commits to the store only; the file's last run writes
//! once (`lan_apply::commit_incoming_ops` says which run is last, and flushes a file whose last run
//! never came because the batch stopped).
//!
//! The store still commits first, and `prev_hash` names the bytes really on disk while a write is
//! owed ([`FileActor::disk_hash`]), so `recover`'s "disk is `prev_hash`: finish the rename" still
//! holds after a crash mid-batch. Any message but a peer batch writes what is owed before it is
//! handled (`actor.rs`), so a client, the watcher and `Replace`'s disk check see the file as before;
//! a save that landed meanwhile is held and merged three-way against the bytes it replaced
//! (`pending_save.rs`), as for any write.

use crate::actor::FileActor;
use crate::expected::Hash;
use crate::handle::ActorError;
use crate::state::DocState;

/// What is on disk while a write is owed: the bytes, their hash and the state they render.
pub(crate) struct Deferred {
    hash: Hash,
    bytes: Vec<u8>,
    state: DocState,
}

impl FileActor {
    /// The hash of the bytes on disk as far as this actor wrote them: the projection, unless a
    /// write is owed. A commit's `prev_hash`.
    pub(crate) fn disk_hash(&self) -> Hash {
        self.deferred.as_ref().map_or(self.hash, |d| d.hash)
    }

    /// Whether a commit's write may wait: not while a save is held or being merged, which have
    /// their own write rules.
    pub(crate) fn can_defer_write(&self) -> bool {
        self.pending_save.is_none() && self.absorbing.is_none()
    }

    /// Owes the write a commit would have made. `before_*` is what the file held before it: kept
    /// only for the first commit of a run of deferred ones, since that is what is on disk.
    pub(crate) fn defer_write(
        &mut self,
        before_hash: Hash,
        before_bytes: Vec<u8>,
        before: DocState,
    ) {
        if self.deferred.is_none() {
            log_write_deferred(&self.cfg.path);
            self.deferred = Some(Deferred {
                hash: before_hash,
                bytes: before_bytes,
                state: before,
            });
        }
        debug_assert!(self.pending_save.is_none());
    }

    /// The commit's own write, from what is really on disk: an owed write's bytes when there is
    /// one, else the commit's `before_*`.
    pub(crate) fn write_after(
        &mut self,
        before_hash: Hash,
        before_bytes: Vec<u8>,
        before: DocState,
    ) -> Result<(), ActorError> {
        match self.deferred.take() {
            Some(d) => self.write_or_hold(d.hash, d.bytes, &d.state),
            None => self.write_or_hold(before_hash, before_bytes, &before),
        }
    }

    /// Writes an owed projection, if any. On a failed write it stays owed and the error goes back.
    pub(crate) fn flush_write(&mut self) -> Result<(), ActorError> {
        let Some(d) = self.deferred.take() else {
            return Ok(());
        };
        let (hash, bytes) = (d.hash, d.bytes.clone());
        self.write_or_hold(hash, bytes, &d.state)
            .inspect_err(|_| self.deferred = Some(d))
    }

    /// [`Self::flush_write`] where no one waits for a reply: a failure is logged and stays owed.
    pub(crate) fn flush_write_logged(&mut self) {
        if let Err(e) = self.flush_write() {
            tracing::warn!(file = %self.cfg.path, error = %e, "deferred_write_failed");
        }
    }
}

/// <https://docs.rs/tracing/latest/tracing/macro.debug.html>
fn log_write_deferred(path: &txtodo_model::FilePath) {
    tracing::debug!(file = %path, "write_deferred");
}
