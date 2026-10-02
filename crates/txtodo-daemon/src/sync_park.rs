//! Peer ops that name a task this document has never held (task partition-converge, lab chaos
//! 20261001-233439). Sync sends each origin device's ops as a run of their own, so with three or
//! more devices an op can land before the insert from another device it builds on: an add after a
//! line a third device added, a move or an edit of it. Such an op used to be skipped for good, and
//! a replay of the log (same order) skipped it again, so a device that pulled a whole history
//! (a fresh one, a rejoin, a Remote mirror) lost it.
//!
//! Now it waits here and is retried after every later commit group that applies something. Its
//! author saw the insert it needs, so that insert is older and on its way. The live sync path and
//! the replay at open run the same function over the same order, so they agree; the replay hands
//! what still waits to the actor. Bounded: past [`MAX_PARKED_OPS`] the oldest is skipped as before.
//!
//! A whole commit group waits, not just the ops that name a missing task (lab chaos
//! 20261003-003120): a1's `do` reordered 21 lines in one commit, some of them a2's adds that b1
//! had not received yet (b1 took a1's run straight from a1, a2's from a2). Applied in part, then
//! finished as the adds came, it left another order than a1 and a2, which applied it whole.

use std::collections::VecDeque;

use std::collections::HashSet;

use crate::state::{DocState, StateError};
use crate::sync_ops::apply_leniently;
use txtodo_model::{Op, OpKind, TaskId};

/// Most ops one document keeps waiting for a task.
pub(crate) const MAX_PARKED_OPS: usize = 10_000;

/// Ops waiting for a task to land, oldest first, each with the error it last gave.
#[derive(Clone, Debug, Default)]
pub(crate) struct Parked {
    ops: VecDeque<(Op, StateError)>,
}

/// What [`apply_parking`] did beyond applying: ops skipped for good, and how many waiting ops
/// landed (the mirror never saw those land).
#[derive(Debug, Default)]
pub(crate) struct Parking {
    pub(crate) skipped: Vec<(Op, StateError)>,
    pub(crate) landed: usize,
}

impl Parked {
    /// How many ops wait (a test seam).
    /// No op waits here: the document is settled as far as this device knows (ADR 0035's digest).
    pub(crate) fn is_empty(&self) -> bool {
        self.ops.is_empty()
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.ops.len()
    }

    /// Retries the waiting ops, one commit group at a time and oldest first, until a round lands
    /// none; returns how many landed. A group still naming a task that is not here keeps waiting
    /// whole; a group that is complete applies, and what of it still fails waits alone, as before.
    fn retry(&mut self, state: &mut DocState) -> usize {
        let mut landed = 0;
        // Bounded: every round but the last lands at least one op.
        loop {
            let before = self.ops.len();
            let waiting: Vec<(Op, StateError)> = self.ops.drain(..).collect();
            for group in waiting.chunk_by(|a, b| a.0.hlc == b.0.hlc) {
                let ops: Vec<Op> = group.iter().map(|(op, _)| op.clone()).collect();
                if needs_a_missing_task(state, &ops) {
                    self.ops.extend(group.iter().cloned());
                    continue;
                }
                let failed = apply_leniently(state, &ops);
                landed += ops.len() - failed.len();
                for op in ops
                    .iter()
                    .filter(|op| !failed.iter().any(|(f, _)| f.id == op.id))
                {
                    log_unparked(op);
                }
                self.ops
                    .extend(failed.into_iter().map(|(op, e)| (op.clone(), e)));
            }
            if self.ops.len() == before {
                return landed;
            }
        }
    }
}

/// Applies `ops` one commit group at a time like `apply_leniently`. An op naming a task with no
/// placement here waits in `parked`; after each group that applied something, the waiting ops are
/// retried. Anything else that does not fit, and the oldest waiting op past the bound, is skipped.
pub(crate) fn apply_parking(state: &mut DocState, ops: &[Op], parked: &mut Parked) -> Parking {
    let mut out = Parking::default();
    for group in ops.chunk_by(|a, b| a.hlc == b.hlc) {
        if needs_a_missing_task(state, group) {
            park_group(parked, group, &mut out);
            continue;
        }
        let failed = apply_leniently(state, group);
        let applied_any = failed.len() < group.len();
        for (op, e) in failed {
            if waits_for_a_task(state, &e) {
                log_parked(op);
                parked.ops.push_back((op.clone(), e));
                if parked.ops.len() > MAX_PARKED_OPS {
                    out.skipped.extend(parked.ops.pop_front());
                }
            } else {
                out.skipped.push((op.clone(), e));
            }
        }
        if applied_any {
            out.landed += parked.retry(state);
        }
    }
    debug_assert!(parked.ops.len() <= MAX_PARKED_OPS);
    out
}

/// Every op of `group` waits, in order (it lands whole, once [`needs_a_missing_task`] says so).
fn park_group(parked: &mut Parked, group: &[Op], out: &mut Parking) {
    for op in group {
        log_parked(op);
        let missing = StateError::UnknownTask(
            task_or_anchor(op).unwrap_or(TaskId::new(txtodo_model::Ulid::from_u128(0))),
        );
        parked.ops.push_back((op.clone(), missing));
        if parked.ops.len() > MAX_PARKED_OPS {
            out.skipped.extend(parked.ops.pop_front());
        }
    }
}

/// Whether some op of `group` names a task (as its subject or its anchor) this document has no
/// placement of, and the group itself does not insert (in any order: a commit can move a line after
/// one it inserts further on): one that has not arrived yet.
fn needs_a_missing_task(state: &DocState, group: &[Op]) -> bool {
    let inserted: HashSet<TaskId> = group
        .iter()
        .filter_map(|op| match &op.kind {
            OpKind::Insert { task, .. } => Some(*task),
            _ => None,
        })
        .collect();
    let missing = |id: &TaskId| !inserted.contains(id) && !state.has_placement(*id);
    group.iter().any(|op| {
        let (subject, anchor) = names(op);
        subject.as_ref().is_some_and(missing) || anchor.as_ref().is_some_and(missing)
    })
}

/// The task an op acts on (none for an insert, which brings its own) and the one it is placed
/// after in this document (a move to another file is placed there, not here).
fn names(op: &Op) -> (Option<TaskId>, Option<TaskId>) {
    match &op.kind {
        OpKind::Insert { after, .. } => (None, *after),
        OpKind::Move {
            task,
            after,
            to_file,
        } => (Some(*task), after.filter(|_| *to_file == op.file)),
        OpKind::SetField { task, .. }
        | OpKind::EditText { task, .. }
        | OpKind::RemoveTag { task, .. } => (Some(*task), None),
        OpKind::BlankInsert { after } | OpKind::BlankRemove { after } => (None, *after),
        OpKind::NotesEdit { .. } => (None, None),
    }
}

/// For a waiting op's recorded reason: the first task it names.
fn task_or_anchor(op: &Op) -> Option<TaskId> {
    let (subject, anchor) = names(op);
    subject.or(anchor)
}

/// Whether `e` names a task this document has no placement of, shown or ghost: one that has not
/// arrived yet. A deleted task keeps a ghost, so an op on it is not this.
fn waits_for_a_task(state: &DocState, e: &StateError) -> bool {
    matches!(e, StateError::UnknownTask(id) if !state.has_placement(*id))
}

/// <https://docs.rs/tracing/latest/tracing/macro.debug.html>
fn log_parked(op: &Op) {
    tracing::debug!(file = %op.file, op = %op.id.ulid(), "sync_op_parked");
}

fn log_unparked(op: &Op) {
    tracing::debug!(file = %op.file, op = %op.id.ulid(), "sync_op_unparked");
}
