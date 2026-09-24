//! A reconcile's ops must replay on a peer (task `sync-poison-op`, 2026-09-25). The reconciler's
//! own ops sometimes do not reproduce its render (`exact: false`, about one reconcile in eight
//! on a busy workspace): a `Move` anchored after a task the same batch inserts later, a task id
//! inserted twice. The actor then adopts the file, which is fine locally, but it used to commit
//! those ops anyway, and a peer, which only has the ops, failed on them on every reconnect.
//!
//! Here the actor synthesizes ops that do replay: delete what is gone, walk the target's tasks
//! top-down (a task already in the right order among the tasks stays, else it moves after its
//! predecessor; a missing one is inserted there), then even out the blank lines after each task.
//! Each op is applied to a scratch copy as it is made, so every anchor exists by construction; the
//! result is kept only if the copy's lines equal the target's. A changed line gets field ops first
//! (`reconcile::change_ops`) and, when those cannot reproduce it, a delete and a re-insert under
//! the same id. A line after a blank takes three ops (insert, blank insert, blank remove): an
//! `Insert` anchors on a task and lands above that task's blanks, which is exactly the single op
//! the reconciler used to commit and a peer then placed on the wrong side of the blank.

use crate::actor::FileActor;
use crate::handle::ActorError;
use crate::reconcile::{Reconciled, change_ops};
use crate::state::{DocState, Entry};
use txtodo_model::{
    DeviceId, Field, FieldValue, FilePath, Hlc, Op, OpId, OpKind, Principal, TaskId, Ulid,
    set_field,
};

/// What a reconcile commits.
pub(crate) struct Settled {
    /// The ops, not yet stamped.
    pub(crate) kinds: Vec<OpKind>,
    /// The state they land on.
    pub(crate) next: DocState,
    /// Replaying `kinds` on the current state renders the target byte for byte.
    pub(crate) exact: bool,
    /// `kinds` were synthesized here, not the reconciler's own.
    pub(crate) synthesized: bool,
}

impl FileActor {
    /// The reconciler's ops when they replay to its render; else synthesized ops that replay to
    /// the same lines; else, when even that fails, the reconciler's ops as before (logged).
    pub(crate) fn settle_reconciled(&self, r: Reconciled) -> Result<Settled, ActorError> {
        let target = r.file.to_bytes();
        if let Some(next) = replay(&self.state, &r.ops)
            && next.to_bytes() == target
        {
            return Ok(Settled {
                kinds: r.ops,
                next,
                exact: true,
                synthesized: false,
            });
        }
        let adopted = DocState::from_file(
            self.cfg.path.clone(),
            &r.file,
            &r.ids,
            self.cfg.identity_mode,
        )?;
        let Some((kinds, work)) = replayable_ops(&self.state, &adopted) else {
            log_not_replayable(&self.cfg.path, r.ops.len());
            return Ok(Settled {
                kinds: r.ops,
                next: adopted,
                exact: false,
                synthesized: false,
            });
        };
        Ok(Settled {
            kinds,
            exact: work.to_bytes() == target,
            next: adopted,
            synthesized: true,
        })
    }
}

fn log_not_replayable(path: &FilePath, ops: usize) {
    tracing::warn!(file = %path, ops, "reconcile_ops_not_replayable");
}

/// `ops` applied in order to a copy of `state`; `None` at the first that does not apply.
fn replay(state: &DocState, ops: &[OpKind]) -> Option<DocState> {
    let mut next = state.clone();
    for kind in ops {
        next.apply(&bare(next.path(), kind.clone())).ok()?;
    }
    Some(next)
}

/// Ops taking `current` to `target`'s lines, and the copy they produced; `None` when no list could
/// reproduce them (a line the op model cannot express, a task id twice in `target`).
pub(crate) fn replayable_ops(
    current: &DocState,
    target: &DocState,
) -> Option<(Vec<OpKind>, DocState)> {
    [Changed::Fields, Changed::Reinsert]
        .into_iter()
        .find_map(|how| synthesize(current, target, how))
}

/// How a task whose line changed is carried over.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Changed {
    /// `reconcile::change_ops`: keeps field-level history.
    Fields,
    /// Delete, then insert the new line under the same id: always exact.
    Reinsert,
}

/// One synthesis pass, each op applied to `work` as it is made.
struct Pass {
    work: DocState,
    ops: Vec<OpKind>,
}

impl Pass {
    fn push(&mut self, kind: OpKind) -> Option<()> {
        self.work
            .apply(&bare(self.work.path(), kind.clone()))
            .ok()?;
        self.ops.push(kind);
        Some(())
    }
}

fn synthesize(
    current: &DocState,
    target: &DocState,
    how: Changed,
) -> Option<(Vec<OpKind>, DocState)> {
    let mut pass = Pass {
        work: current.clone(),
        ops: Vec::new(),
    };
    delete_and_change(&mut pass, current, target, how)?;
    place_tasks(&mut pass, target)?;
    even_blanks(&mut pass, target)?;
    let same = pass.work.len() == target.len()
        && (0..target.len()).all(|i| pass.work.entry_at(i) == target.entry_at(i));
    debug_assert!(pass.ops.len() <= 3 * (current.len() + target.len()) + 1);
    same.then_some((pass.ops, pass.work))
}

/// Tasks `target` lacks are deleted; a task whose line changed gets field ops, or with
/// [`Changed::Reinsert`] is deleted here and inserted again by [`place_tasks`].
fn delete_and_change(
    pass: &mut Pass,
    current: &DocState,
    target: &DocState,
    how: Changed,
) -> Option<()> {
    for (id, line) in current.task_lines() {
        let Some(new) = target.line_of(id) else {
            pass.push(delete(id)?)?;
            continue;
        };
        if new == *line {
            continue;
        }
        if how == Changed::Reinsert {
            pass.push(delete(id)?)?;
            continue;
        }
        for kind in change_ops(line, &new, id) {
            pass.push(kind)?;
        }
    }
    Some(())
}

fn delete(id: TaskId) -> Option<OpKind> {
    set_field(id, Field::Deleted, FieldValue::Bool(true)).ok()
}

fn is_blank(state: &DocState, i: usize) -> bool {
    matches!(state.entry_at(i), Some(Entry::Blank(_)))
}

/// Top-down: the `k`-th target task must have exactly `k` tasks before it (the first `k`, placed
/// already); if not it moves right after the previous one, and a missing one is inserted there.
fn place_tasks(pass: &mut Pass, target: &DocState) -> Option<()> {
    let mut prev: Option<TaskId> = None;
    let path = target.path().clone();
    for (k, (id, line)) in target.task_lines().enumerate() {
        match pass.work.index_of(id) {
            Some(at) if tasks_before(&pass.work, at) == k => {}
            Some(_) => pass.push(OpKind::Move {
                task: id,
                after: prev,
                to_file: path.clone(),
            })?,
            None => pass.push(OpKind::Insert {
                task: id,
                after: prev,
                line: line.raw()?.to_owned(),
            })?,
        }
        prev = Some(id);
    }
    Some(())
}

fn tasks_before(state: &DocState, at: usize) -> usize {
    (0..at).filter(|&i| !is_blank(state, i)).count()
}

/// With the tasks in order, each run of blanks (the leading one, then the one after each task)
/// gets `BlankInsert`s or `BlankRemove`s after its task until its length matches the target's.
fn even_blanks(pass: &mut Pass, target: &DocState) -> Option<()> {
    let anchors = std::iter::once(None).chain(target.task_lines().map(|(id, _)| Some(id)));
    for after in anchors.collect::<Vec<_>>() {
        let want = blanks_after(target, after)?;
        // Bounded by the document length: each turn changes the run by one.
        loop {
            let have = blanks_after(&pass.work, after)?;
            let op = match have.cmp(&want) {
                std::cmp::Ordering::Equal => break,
                std::cmp::Ordering::Less => OpKind::BlankInsert { after },
                std::cmp::Ordering::Greater => OpKind::BlankRemove { after },
            };
            pass.push(op)?;
        }
    }
    Some(())
}

/// How many blanks follow `after` (the task, or the top of the document for `None`).
fn blanks_after(state: &DocState, after: Option<TaskId>) -> Option<usize> {
    let from = match after {
        None => 0,
        Some(id) => state.index_of(id)? + 1,
    };
    Some(
        (from..state.len())
            .take_while(|&i| is_blank(state, i))
            .count(),
    )
}

/// `kind` under a zero stamp, for a scratch copy only: `DocState::apply` never reads the stamp.
fn bare(path: &FilePath, kind: OpKind) -> Op {
    let zero = DeviceId::new(Ulid::from_u128(0));
    Op {
        id: OpId::new(Ulid::from_u128(0)),
        hlc: Hlc::zero(zero),
        principal: Principal::External { device: zero },
        file: path.clone(),
        kind,
    }
}
