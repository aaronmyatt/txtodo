//! The actor's sync-facing messages (plan M4): list the open needs_review flags, resolve one,
//! the digest, a peer's ops. Same `impl FileActor`; split from `actor.rs` for the file budget.
//! The Loro import path that raised flags is gone (ADR 0038), so no new flag is raised.

use crate::actor::{Commit, CommitTail, FileActor};
use crate::handle::{ActorError, ActorMsg, Applied, ConflictRow, FileConflicts, Resolution};
use crate::mutation::{TaskRef, resolve};
use crate::reconcile::change_ops;
use txtodo_core::Edit;
use txtodo_model::{Principal, TaskId};

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
            ActorMsg::Version { reply } => {
                let _ = reply.send(self.mirror.version());
            }
            ActorMsg::Export { since, reply } => {
                let _ = reply.send(self.on_export(&since));
            }
            ActorMsg::Resolve {
                task,
                resolution,
                principal,
                reply,
            } => {
                let _ = reply.send(self.on_resolve(task, resolution, principal));
            }
            ActorMsg::SyncOps { ops, reply } => {
                let _ = reply.send(self.on_sync_ops(ops));
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

    /// Resolves one flag: `Merged` keeps the file; `Mine`/`Theirs` write that side's description
    /// back as field ops. The write-back and the clear are one store transaction.
    pub(crate) fn on_resolve(
        &mut self,
        task: TaskRef,
        resolution: Resolution,
        principal: Principal,
    ) -> Result<Applied, ActorError> {
        let (_, id) = resolve(&self.state, &task)?;
        let flag = self
            .lock_store()
            .open_flags(&self.cfg.path)?
            .into_iter()
            .find(|r| r.task == id)
            .ok_or(ActorError::NoFlag(id))?;
        let target = match resolution {
            Resolution::Merged => None,
            Resolution::Mine => Some(flag.mine),
            Resolution::Theirs => Some(flag.theirs),
        };
        let kinds = match target {
            None => Vec::new(),
            Some(text) => self.description_ops(id, &text)?,
        };
        let ops = self.stamp(kinds, &principal)?;
        let mut next = self.state.clone();
        for op in &ops {
            next.apply(op)?;
        }
        let bytes = next.to_bytes();
        let write = bytes != self.projection;
        let change = self.commit(Commit {
            ops,
            next,
            bytes,
            write,
            snapshot: false,
            tail: CommitTail {
                review: Vec::new(),
                flush: true,
                clear: Some((id, self.clock.now_ms())),
                persist_mirror: false,
                source: None,
            },
        })?;
        debug_assert!(self.on_conflicts()?.flags.iter().all(|c| c.row.task != id));
        Ok(Applied {
            applied: u32::try_from(change.ops.len()).unwrap_or(u32::MAX),
            hash: change.hash,
            hlc: self.hlc,
        })
    }

    /// The field ops that give `task` the description `text` (bytes from a flag).
    fn description_ops(
        &self,
        task: TaskId,
        text: &[u8],
    ) -> Result<Vec<txtodo_model::OpKind>, ActorError> {
        let want = std::str::from_utf8(text)
            .map_err(|_| ActorError::Unsupported("a flag side that is not UTF-8"))?;
        let line = self.state.line_of(task).ok_or(ActorError::Unsupported(
            "resolving a task that left the file",
        ))?;
        let edit = Edit::new()
            .set_description(want)
            .map_err(|_| ActorError::Unsupported("a flag side with a line break"))?;
        let new_line = txtodo_core::apply(&line, &edit);
        let kinds = change_ops(&line, &new_line, task);
        debug_assert!(
            kinds
                .iter()
                .all(|k| crate::convert::task_of(k) == Some(task))
        );
        Ok(kinds)
    }
}
