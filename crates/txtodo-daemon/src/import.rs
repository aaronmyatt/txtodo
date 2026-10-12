//! The actor's sync-facing messages (plan M4): list the open needs_review flags,
//! the digest, a peer's ops. Same `impl FileActor`; split from `actor.rs` for the file budget.
//! The Loro import path that raised flags is gone (ADR 0038), so no new flag is raised.

use crate::actor::FileActor;
use crate::handle::{ActorError, ActorMsg, ConflictRow, FileConflicts};

impl FileActor {
    /// The sync/conflict-resolution messages — split out of `actor.rs::handle_core` purely to
    /// keep that function's line count in budget. The wildcard covers every variant
    /// `handle`/`handle_core` already consumed (never actually reached here).
    pub(crate) fn handle_sync(&mut self, msg: ActorMsg) {
        match msg {
            ActorMsg::Conflicts { reply } => {
                let _ = reply.send(self.on_conflicts());
            }
            ActorMsg::Digest { reply } => {
                let _ = reply.send(self.on_digest());
            }
            #[cfg(test)]
            ActorMsg::SkewDigestForTest { on, reply } => {
                self.skew_digest = on;
                let _ = reply.send(());
            }
            ActorMsg::SyncOps { ops, defer, reply } => {
                let _ = reply.send(self.on_sync_run(ops, defer));
            }
            ActorMsg::MigrateToSidecar { dry_run, reply } => {
                let _ = reply.send(self.on_migrate_to_sidecar(dry_run));
            }
            _ => {}
        }
    }

    /// ADR 0035: the two hashes a `Digest` compares; `None` with peer ops waiting, since a document
    /// about to change says nothing about a split.
    fn on_digest(&self) -> Option<crate::sync_digest::DocDigest> {
        #[cfg(test)]
        let bytes = if self.skew_digest {
            crate::actor::hash_of(&[self.projection.as_slice(), b"skewed"].concat())
        } else {
            self.hash
        };
        #[cfg(not(test))]
        let bytes = self.hash;
        self.parked
            .is_empty()
            .then_some(crate::sync_digest::DocDigest {
                ops: self.op_set,
                bytes,
            })
    }

    /// Open flags with the line each task sits on now (0 when it is no longer in the file).
    pub(crate) fn on_conflicts(&self) -> Result<FileConflicts, ActorError> {
        let rows = self.lock_store().open_flags(&self.cfg.path)?;
        let out: Vec<ConflictRow> = rows
            .into_iter()
            .map(|row| ConflictRow {
                line_number: self.state.index_of(row.task).map_or(0, |i| i + 1),
                row,
            })
            .collect();
        debug_assert!(out.iter().all(|c| c.row.file == self.cfg.path));
        Ok(FileConflicts {
            flags: out,
            duplicates: crate::duplicates::duplicate_groups(&self.state),
        })
    }
}
