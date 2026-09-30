//! A save on disk the actor has not merged yet is never written over (task editor-save-lost,
//! `tasks/editor-save-lost/notes.md`). Same `impl FileActor`, split out for the file budget.
//!
//! The watcher reports a save only after its debounce, so a commit can land first (a CLI edit, a
//! peer's ops). It used to write its render straight over the save, and the watcher's event then
//! found the daemon's own bytes and skipped the reconcile: the save was gone. Now a commit that
//! finds the file is not its last write (nor a recent one, nor bytes it is merging) lands in the
//! store and the state but leaves the file alone, keeping what it last wrote as the base. The
//! watcher's event then merges three-way (design §4.3 step 3: the state "may already be ahead" of
//! the bytes last written): the editor's changes, base → disk, applied on the current state, and
//! one write. Folding the disk in before every write instead would skip the debounce and could
//! read a save written in place half-way through.

use std::sync::Arc;
use std::time::{Duration, SystemTime};

use crate::actor::{Commit, CommitTail, FileActor, hash_of};
use crate::handle::ActorError;
use crate::reconcile::reconcile;
use crate::reconcile_sidecar::{Side, reconcile_sidecar};
use crate::state::{DocState, scratch_op};
use crate::sync_ops::apply_leniently;
use crate::write::read_or_empty;
use txtodo_core::parse_file;
use txtodo_model::{CostWeights, IdentityMode, Op, OpKind, Principal, TaskId};

/// A save the actor stopped writing over: `base` is what it last wrote there (the common ancestor
/// of the three-way merge) and `base_ids` that text's task ids, for sidecar identity.
pub(crate) struct PendingSave {
    base: Vec<u8>,
    base_ids: Vec<Option<TaskId>>,
}

/// A pending save whose file has sat still this long is merged after any message, in case the
/// watcher's event never comes; the event itself normally arrives after 150 ms.
const SETTLED: Duration = Duration::from_secs(1);

impl FileActor {
    /// `commit_inner`'s gate, asked before the state moves on: may this commit write the file?
    /// No while a save is pending. Else yes when the file holds our last write, a recent one, or
    /// the bytes this commit merges; anything else is a save we have not merged, so it becomes
    /// pending with our current projection as its base.
    pub(crate) fn may_write(&mut self) -> Result<bool, ActorError> {
        if self.pending_save.is_some() {
            return Ok(false);
        }
        let disk = hash_of(&read_or_empty(&self.cfg.disk)?);
        let now = self.clock.now_instant();
        if disk == self.hash || self.absorbing == Some(disk) || self.expected.is_ours(&disk, now) {
            return Ok(true);
        }
        self.pending_save = Some(PendingSave {
            base: self.projection.clone(),
            base_ids: self.state.line_ids().collect(),
        });
        log_write_held(&self.cfg.path);
        Ok(false)
    }

    /// The watcher's event for a file with a pending save: merges it, or, when the file holds our
    /// base again (the save was undone), writes what the state has now.
    pub(crate) fn merge_pending_save(&mut self) -> Result<(), ActorError> {
        let Some(pending) = self.pending_save.take() else {
            return Ok(());
        };
        let disk = read_or_empty(&self.cfg.disk)?;
        if disk == pending.base {
            self.write_projection_and_log(self.hash)?;
            return Ok(());
        }
        let ops = self.save_ops(&pending, &disk)?;
        let mut next = self.state.clone();
        for (op, e) in apply_leniently(&mut next, &ops) {
            log_save_op_skipped(op, &e);
        }
        let bytes = next.to_bytes();
        self.absorbing = Some(hash_of(&disk));
        let committed = self.commit(Commit {
            ops,
            next,
            bytes,
            write: true,
            snapshot: false,
            tail: CommitTail {
                source: Some("external".to_owned()),
                ..CommitTail::default()
            },
        });
        self.absorbing = None;
        debug_assert!(committed.is_err() || self.pending_save.is_none());
        committed.map(|_| ())
    }

    /// The editor's changes as this device's stamped ops: base → disk, reconciled the same way
    /// as any external edit.
    fn save_ops(&mut self, pending: &PendingSave, disk: &[u8]) -> Result<Vec<Op>, ActorError> {
        let (old, new) = (parse_file(&pending.base), parse_file(disk));
        let clock = Arc::clone(&self.clock);
        let mut mint = || TaskId::new(clock.new_ulid());
        let r = match self.cfg.identity_mode {
            IdentityMode::Tagged => reconcile(&old, &new, &self.cfg.path, &mut mint),
            IdentityMode::Sidecar => reconcile_sidecar(
                Side {
                    file: &old,
                    ids: &pending.base_ids,
                },
                &new,
                &self.cfg.path,
                &CostWeights::DEFAULT,
                &mut mint,
            ),
        };
        let kinds = reanchor(r.ops, &r.ids, &self.state);
        let principal = Principal::External {
            device: self.cfg.device,
        };
        self.stamp(kinds, &principal)
    }

    /// After any message: a pending save whose file has sat still for [`SETTLED`] is merged now,
    /// in case the watcher's event was lost. A failure is logged; the next message tries again.
    pub(crate) fn merge_settled_save(&mut self) {
        if self.pending_save.is_none() || !settled(&self.cfg.disk) {
            return;
        }
        if let Err(e) = self.merge_pending_save() {
            crate::external::tracing_stub_error(&self.cfg.path, &e);
        }
    }
}

/// The save's inserts and moves are anchored on the line above them in the editor's file, but the
/// state may have lost that line meanwhile (a peer deleted it). Such an op would be skipped and a
/// line the editor wrote would be gone, so it is re-anchored on the nearest line above it in the
/// editor's file that the state still has (or the top). `ids` is the editor's file, line by line.
/// Other ops on a line the state no longer has still fail and are skipped: the delete wins.
fn reanchor(kinds: Vec<OpKind>, ids: &[Option<TaskId>], state: &DocState) -> Vec<OpKind> {
    let mut scratch = state.clone();
    let mut out = Vec::with_capacity(kinds.len());
    for mut kind in kinds {
        if let OpKind::Insert { task, after, .. } | OpKind::Move { task, after, .. } = &mut kind
            && after.is_some_and(|a| scratch.index_of(a).is_none())
        {
            *after = nearest_kept_above(*task, ids, &scratch);
        }
        // Scratch only tracks which lines exist by now; a refusal changes nothing here.
        let _ = scratch.apply(&scratch_op(scratch.path(), kind.clone()));
        out.push(kind);
    }
    debug_assert_eq!(out.len(), out.capacity());
    out
}

/// The closest task above `task` in the editor's file that `state` holds; `None` is the top.
fn nearest_kept_above(task: TaskId, ids: &[Option<TaskId>], state: &DocState) -> Option<TaskId> {
    let at = ids.iter().position(|id| *id == Some(task))?;
    ids[..at]
        .iter()
        .rev()
        .flatten()
        .copied()
        .find(|id| state.index_of(*id).is_some())
}

/// True when the file was last modified at least [`SETTLED`] ago, or cannot be read.
fn settled(path: &std::path::Path) -> bool {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| SystemTime::now().duration_since(t).ok())
        .is_none_or(|age| age >= SETTLED)
}

/// tracing: <https://docs.rs/tracing/latest/tracing/macro.info.html>
fn log_write_held(path: &txtodo_model::FilePath) {
    tracing::info!(file = %path, "write_held_for_unmerged_save");
}

/// An edit from the save that no longer fits the state (a peer changed or deleted that line in
/// the same moment): the state keeps its version.
fn log_save_op_skipped(op: &Op, e: &crate::state::StateError) {
    tracing::warn!(file = %op.file, op = %op.id.ulid(), error = %e, "save_op_skipped");
}
