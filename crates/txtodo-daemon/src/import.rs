//! The actor's sync-facing messages (plan M4): the duplicate groups, the digest, a peer's ops.
//! Same `impl FileActor`; split from `actor.rs` for the file budget. Same-word review (the
//! `needs_review` flags and their resolve) went with the Loro import path (ADR 0038).

use crate::actor::FileActor;
use crate::duplicates::DuplicateGroup;
use crate::handle::{ActorError, ActorMsg};

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

    /// The file's duplicate groups (ADR 0032), derived from the state now.
    pub(crate) fn on_conflicts(&self) -> Result<Vec<DuplicateGroup>, ActorError> {
        Ok(crate::duplicates::duplicate_groups(&self.state))
    }
}
