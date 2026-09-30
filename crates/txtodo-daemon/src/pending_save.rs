//! A save on disk the actor has not merged yet is never written over (task editor-save-lost,
//! `tasks/editor-save-lost/notes.md`). Same `impl FileActor`, split out for the file budget.
//!
//! The watcher reports a save only after its debounce, so a commit can land first (a CLI edit, a
//! peer's ops). It used to write its render straight over the save, and the watcher's event then
//! found the daemon's own bytes and skipped the reconcile: the save was gone. Now the write checks
//! the file right before its rename (temp file written and synced): unless it is our last write, a
//! recent one, or the bytes this commit merges, the rename is dropped. The commit still lands in
//! the store and the state; what we last wrote is kept as the base. The watcher's event then
//! merges three-way (design §4.3 step 3: the state "may already be ahead" of the bytes last
//! written): the editor's changes, base → disk, applied on the current state, and one write.
//! Folding the disk in before every write instead would skip the debounce and could read a save
//! written in place half-way through. A check before the commit instead of before the rename left
//! the SQLite commit and the fsync between check and rename: the lab still lost saves there.

use std::sync::Arc;
use std::time::{Duration, SystemTime};

use crate::actor::{Commit, CommitTail, FileActor, hash_of};
use crate::expected::Hash;
use crate::handle::ActorError;
use crate::reconcile::reconcile;
use crate::reconcile_sidecar::{Side, reconcile_sidecar};
use crate::state::{DocState, scratch_op};
use crate::sync_ops::apply_leniently;
use crate::write::{read_or_empty, write_atomic_if};
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
    /// `commit_inner`'s write, after the state moved on: `ours` is the hash of what we last put
    /// on disk, `before` the state and bytes it rendered. While a save is pending, and unless this
    /// commit is the merge, nothing is written. Else the new projection replaces the file only if,
    /// right before the rename, the file is still ours; otherwise the save it holds becomes
    /// pending, based on `before` (an older base, if one is already pending, is kept).
    pub(crate) fn write_or_hold(
        &mut self,
        ours: Hash,
        before_bytes: Vec<u8>,
        before: &DocState,
    ) -> Result<(), ActorError> {
        let merging = self.absorbing;
        let wrote = (self.pending_save.is_none() || merging.is_some())
            && self.write_projection_if_ours(ours)?;
        if wrote {
            self.settle_held()?;
        } else if self.pending_save.is_none() {
            self.hold(PendingSave {
                base: before_bytes,
                base_ids: before.line_ids().collect(),
            })?;
            log_write_held(&self.cfg.path);
        }
        debug_assert!(wrote != self.pending_save.is_some());
        Ok(())
    }

    /// Holds writes for `pending`, and keeps its base in the store: a restart before the merge
    /// resumes it three-way (`restore_held`) instead of reading the held ops as deleted lines.
    fn hold(&mut self, pending: PendingSave) -> Result<(), ActorError> {
        let encoded = encode_held(&pending);
        let key = held_key(&self.cfg.path);
        self.lock_store().meta_set(&key, &encoded)?;
        self.pending_save = Some(pending);
        Ok(())
    }

    /// The file holds our state again: nothing is held.
    fn settle_held(&mut self) -> Result<(), ActorError> {
        if self.pending_save.take().is_some() {
            let key = held_key(&self.cfg.path);
            self.lock_store().meta_set(&key, &[])?;
        }
        Ok(())
    }

    /// At open, before the disk is compared with the projection: a base a crash left held.
    pub(crate) fn restore_held(&mut self) -> Result<(), ActorError> {
        let key = held_key(&self.cfg.path);
        let stored = self.lock_store().meta_get(&key)?;
        self.pending_save = stored.as_deref().and_then(decode_held);
        Ok(())
    }

    /// At open, when the file turned out to hold our projection after all.
    pub(crate) fn forget_held(&mut self) -> Result<(), ActorError> {
        self.settle_held()
    }

    /// Writes the projection unless, checked right before the rename, the file holds neither
    /// `ours`, a recent write of ours, nor the bytes being merged.
    fn write_projection_if_ours(&mut self, ours: Hash) -> Result<bool, ActorError> {
        let now = self.clock.now_instant();
        self.expected.arm(self.hash, now);
        let (disk, merging, expected) = (&self.cfg.disk, self.absorbing, &mut self.expected);
        let still_ours = || {
            let found = hash_of(&read_or_empty(disk)?);
            Ok(found == ours || Some(found) == merging || expected.is_ours(&found, now))
        };
        let wrote = write_atomic_if(disk, &self.projection, still_ours)?;
        if wrote {
            self.writes_total += 1;
            self.cfg.stats.count_write();
            log_projection_written(&self.cfg.path, self.projection.len(), &self.hash);
        }
        Ok(wrote)
    }

    /// The watcher's event for a file with a pending save: merges it, or, when the file holds our
    /// base again (the save was undone), writes what the state has now. The base stays pending
    /// until the merge's own write lands, so a second save racing the merge is merged next time.
    pub(crate) fn merge_pending_save(&mut self) -> Result<(), ActorError> {
        let Some(pending) = self.pending_save.as_ref() else {
            return Ok(());
        };
        let (base, base_ids) = (pending.base.clone(), pending.base_ids.clone());
        let disk = read_or_empty(&self.cfg.disk)?;
        if disk == base {
            if self.write_projection_if_ours(hash_of(&base))? {
                self.settle_held()?;
            }
            return Ok(());
        }
        let (ops, merged_ids) = self.save_ops(&base, &base_ids, &disk)?;
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
        // Another save landed during this write: what we just merged is the next merge's base,
        // or its lines would be merged in twice.
        if self.pending_save.is_some() {
            self.hold(PendingSave {
                base: disk,
                base_ids: merged_ids,
            })?;
        }
        committed.map(|_| ())
    }

    /// The editor's changes as this device's stamped ops: base → disk, reconciled the same way
    /// as any external edit; and the task ids of the disk's lines as merged.
    fn save_ops(
        &mut self,
        base: &[u8],
        base_ids: &[Option<TaskId>],
        disk: &[u8],
    ) -> Result<(Vec<Op>, Vec<Option<TaskId>>), ActorError> {
        let (old, new) = (parse_file(base), parse_file(disk));
        let clock = Arc::clone(&self.clock);
        let mut mint = || TaskId::new(clock.new_ulid());
        let r = match self.cfg.identity_mode {
            IdentityMode::Tagged => reconcile(&old, &new, &self.cfg.path, &mut mint),
            IdentityMode::Sidecar => reconcile_sidecar(
                Side {
                    file: &old,
                    ids: base_ids,
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
        Ok((self.stamp(kinds, &principal)?, r.ids))
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

/// The same event `write_projection_and_log` emits, so the lab and logs read one name.
fn log_projection_written(path: &txtodo_model::FilePath, bytes: usize, hash: &Hash) {
    tracing::info!(file = %path, bytes, hash = %crate::expected::hex8(hash), "projection_written");
}

/// The store's meta key for a document's held base.
fn held_key(path: &txtodo_model::FilePath) -> String {
    format!("held_base/{path}")
}

/// A held base as bytes: the id count, one id per line (`-` for a blank), then the base itself.
/// Empty means nothing is held.
fn encode_held(pending: &PendingSave) -> Vec<u8> {
    let mut out = format!("{}\n", pending.base_ids.len()).into_bytes();
    for id in &pending.base_ids {
        let line = id.map_or_else(|| "-".to_owned(), |t| t.to_string());
        out.extend_from_slice(line.as_bytes());
        out.push(b'\n');
    }
    out.extend_from_slice(&pending.base);
    debug_assert!(!out.is_empty());
    out
}

/// `encode_held`'s inverse; `None` for empty or unreadable bytes (then nothing is held).
fn decode_held(bytes: &[u8]) -> Option<PendingSave> {
    let mut rest = bytes;
    let mut next_line = || {
        let end = rest.iter().position(|b| *b == b'\n')?;
        let line = std::str::from_utf8(&rest[..end]).ok()?.to_owned();
        rest = &rest[end + 1..];
        Some(line)
    };
    let count: usize = next_line()?.parse().ok()?;
    let mut base_ids = Vec::with_capacity(count.min(crate::state::MAX_LINES_PER_FILE));
    for _ in 0..count {
        let line = next_line()?;
        base_ids.push(match line.as_str() {
            "-" => None,
            id => Some(TaskId::new(txtodo_model::Ulid::parse(id)?)),
        });
    }
    Some(PendingSave {
        base: rest.to_vec(),
        base_ids,
    })
}
