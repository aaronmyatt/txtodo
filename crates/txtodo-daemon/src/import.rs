//! The actor's sync-facing paths (plan M4): import a peer's Loro updates, list the open
//! needs_review flags, resolve one. Same `impl FileActor`; split from `actor.rs` for the file
//! budget. An import merges in the mirror first (that is where the CRDT lives), derives the ops
//! that take the state from before to after, applies them to a clone of the state and commits
//! them with the mirror snapshot in the same transaction. Flags are raised from that commit only
//! — never on the local edit path.

use std::sync::Arc;

use crate::actor::{Commit, CommitTail, FileActor};
use crate::handle::{ActorError, Applied, ConflictRow, Resolution};
use crate::mirror::file_like;
use crate::mutation::{TaskRef, resolve};
use crate::reconcile::change_ops;
use crate::state::DocState;
use txtodo_core::Edit;
use txtodo_crdt::Stamp;
use txtodo_model::{DeviceId, OpId, Principal, TaskId};
use txtodo_store::ReviewRow;

impl FileActor {
    /// Imports `updates` from `peer`. Ops derived from the merge are stamped with one local tick
    /// and `Principal::User { device: peer }`; same-word conflicts become flags on the change.
    pub(crate) fn on_import(
        &mut self,
        updates: Vec<u8>,
        peer: DeviceId,
    ) -> Result<Applied, ActorError> {
        let imported = self
            .mirror
            .import(&updates)
            .map_err(|e| ActorError::Mirror(e.to_string()))?;
        if !imported.applied {
            return Ok(Applied {
                applied: 0,
                hash: self.hash,
                hlc: self.hlc,
            });
        }
        let hlc = self.tick()?;
        let stamp = Stamp {
            hlc,
            principal: Principal::User { device: peer },
        };
        let clock = Arc::clone(&self.clock);
        let mut mint = || OpId::new(clock.new_ulid());
        let ops = self
            .mirror
            .ops_between(&imported, &stamp, &mut mint)
            .map_err(|e| ActorError::Mirror(e.to_string()))?;
        let next = self.state_after(&ops)?;
        let review = self
            .mirror
            .review(&imported)
            .map_err(|e| ActorError::Mirror(e.to_string()))?;
        let rows = review_rows(&review, self.clock.now_ms());
        let bytes = next.to_bytes();
        let write = bytes != self.projection;
        let change = self.commit(Commit {
            ops,
            next,
            bytes,
            write,
            snapshot: false,
            tail: CommitTail {
                review: rows,
                flush: false,
                clear: None,
                persist_mirror: true,
            },
        })?;
        debug_assert!(
            self.mirror.agrees_with(&self.state),
            "import left mirror in step"
        );
        Ok(Applied {
            applied: u32::try_from(change.ops.len()).unwrap_or(u32::MAX),
            hash: change.hash,
            hlc,
        })
    }

    /// The state with the derived ops applied; if the state cannot take them, the mirror's
    /// canonical rendering becomes the state (bytes over quirks — convergence first).
    fn state_after(&self, ops: &[txtodo_model::Op]) -> Result<DocState, ActorError> {
        let mut next = self.state.clone();
        let mut failed = None;
        for op in ops {
            if let Err(e) = next.apply(op) {
                failed = Some(e);
                break;
            }
        }
        let Some(e) = failed else {
            return Ok(next);
        };
        tracing::warn!(file = %self.cfg.path, error = %e, "import_ops_refused_adopting_mirror");
        let file = file_like(&self.mirror.canonical_bytes(&self.state), &self.state);
        let adopted = DocState::from_file(self.cfg.path.clone(), &file)?;
        debug_assert!(self.mirror.agrees_with(&adopted));
        Ok(adopted)
    }

    /// Open flags with the line each task sits on now (0 when it is no longer in the file).
    pub(crate) fn on_conflicts(&self) -> Result<Vec<ConflictRow>, ActorError> {
        let rows = self.lock_store().open_flags(&self.cfg.path)?;
        let out: Vec<ConflictRow> = rows
            .into_iter()
            .map(|row| ConflictRow {
                line_number: self.state.index_of(row.task).map_or(0, |i| i + 1),
                row,
            })
            .collect();
        debug_assert!(out.iter().all(|c| c.row.file == self.cfg.path));
        Ok(out)
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
            },
        })?;
        debug_assert!(self.on_conflicts()?.iter().all(|c| c.row.task != id));
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
        let kinds = change_ops(&line, &new_line);
        debug_assert!(
            kinds
                .iter()
                .all(|k| crate::convert::task_of(k) == Some(task))
        );
        Ok(kinds)
    }
}

/// Flags as store rows, all raised `now`; an over-cap file is logged, not flagged per task.
fn review_rows(review: &txtodo_crdt::Review, now: u64) -> Vec<ReviewRow> {
    for file in &review.overflowed {
        tracing::warn!(file = %file, "review_flags_overflowed");
    }
    let rows: Vec<ReviewRow> = review
        .flags
        .iter()
        .map(|f| ReviewRow {
            file: f.file.clone(),
            task: f.task,
            raised_at_ms: now,
            mine: f.mine.clone().into_bytes(),
            theirs: f.theirs.clone().into_bytes(),
        })
        .collect();
    debug_assert_eq!(rows.len(), review.flags.len());
    debug_assert!(rows.iter().all(|r| r.raised_at_ms == now));
    rows
}
