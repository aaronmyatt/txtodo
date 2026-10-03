//! Peer ops that name a task this document has never held (task partition-converge, lab chaos
//! 20261001-233439). Sync sends each origin device's ops as a run of their own, so with three or
//! more devices an op can land before the insert from another device it builds on: an add after a
//! line a third device added, a move or an edit of it. Such an op used to be skipped for good, and
//! a replay of the log (same order) skipped it again, so a device that pulled a whole history
//! (a fresh one, a rejoin, a Remote mirror) lost it.
//!
//! Now it waits here and is retried once a later commit group touches a task it names. Its author
//! saw the insert it needs, so that insert is older and on its way. The live sync path and the
//! replay at open run the same function over the same order, so they agree; the replay hands what
//! still waits to the actor. Bounded: past [`MAX_PARKED_OPS`] the oldest is skipped, with a warn.
//!
//! A whole commit group waits, not just the ops that name a missing task (lab chaos
//! 20261003-003120): a1's `do` reordered 21 lines in one commit, some of them a2's adds that b1
//! had not received yet (b1 took a1's run straight from a1, a2's from a2). Applied in part, then
//! finished as the adds came, it left another order than a1 and a2, which applied it whole.
//!
//! A text edit that does not fit waits too (task first-sync-speed): it may build on another
//! device's edit of the same line that has not arrived, as notes.md edits do (9efd4c74).
//!
//! Each waiting op is indexed by the tasks it names, so a group that lands retries only the groups
//! naming a task it touched, not the whole queue (task first-sync-speed).

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use crate::state::{DocState, StateError};
use crate::sync_ops::apply_leniently;
use txtodo_model::{Op, OpKind, TaskId};

/// Most ops one document keeps waiting for a task.
pub(crate) const MAX_PARKED_OPS: usize = 10_000;

/// Ops waiting for a task to land or change, by arrival number (oldest first: one commit
/// group's ops are consecutive), each with the error it last gave.
#[derive(Clone, Debug, Default)]
pub(crate) struct Parked {
    ops: BTreeMap<u64, (Op, StateError)>,
    next: u64,
    /// The arrival numbers of the waiting ops that name each task.
    by_task: HashMap<TaskId, BTreeSet<u64>>,
}

/// What [`apply_parking`] did beyond applying: ops skipped for good, and how many waiting ops
/// landed (the mirror never saw those land).
#[derive(Debug, Default)]
pub(crate) struct Parking {
    pub(crate) skipped: Vec<(Op, StateError)>,
    pub(crate) landed: usize,
}

impl Parked {
    /// No op waits here: the document is settled as far as this device knows (ADR 0035's digest).
    pub(crate) fn is_empty(&self) -> bool {
        self.ops.is_empty()
    }

    /// How many ops wait (a test seam).
    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.ops.len()
    }

    /// Waits `op` at number `n`, indexed by the tasks it names.
    fn put(&mut self, n: u64, op: Op, e: StateError) {
        for task in names(&op).into_iter().flatten() {
            self.by_task.entry(task).or_default().insert(n);
        }
        self.ops.insert(n, (op, e));
    }

    /// Waits `op` last; past [`MAX_PARKED_OPS`] the oldest goes to `skipped`.
    fn push(&mut self, op: Op, e: StateError, skipped: &mut Vec<(Op, StateError)>) {
        log_parked(&op);
        self.next += 1;
        self.put(self.next, op, e);
        if self.ops.len() > MAX_PARKED_OPS {
            let oldest = self.ops.keys().next().copied();
            if let Some(dropped) = oldest.and_then(|n| self.take(n)) {
                log_overflow(&dropped.0);
                skipped.push(dropped);
            }
        }
        debug_assert!(self.ops.len() <= MAX_PARKED_OPS);
    }

    fn take(&mut self, n: u64) -> Option<(Op, StateError)> {
        let (op, e) = self.ops.remove(&n)?;
        for task in names(&op).into_iter().flatten() {
            if let Some(waiting) = self.by_task.get_mut(&task) {
                waiting.remove(&n);
                if waiting.is_empty() {
                    self.by_task.remove(&task);
                }
            }
        }
        Some((op, e))
    }

    /// The waiting ops that name one of `tasks`.
    fn naming(&self, tasks: &HashSet<TaskId>) -> BTreeSet<u64> {
        tasks
            .iter()
            .filter_map(|t| self.by_task.get(t))
            .flatten()
            .copied()
            .collect()
    }

    /// The numbers of the commit group `n` is in: its waiting neighbours with the same stamp.
    fn group_of(&self, n: u64) -> Vec<u64> {
        let Some((op, _)) = self.ops.get(&n) else {
            return Vec::new();
        };
        let same = |(_, (o, _)): &(&u64, &(Op, StateError))| o.hlc == op.hlc;
        let mut group: Vec<u64> = self
            .ops
            .range(..n)
            .rev()
            .take_while(same)
            .map(|(k, _)| *k)
            .collect();
        group.reverse();
        group.extend(self.ops.range(n..).take_while(same).map(|(k, _)| *k));
        debug_assert!(group.contains(&n));
        group
    }

    /// Retries the waiting groups naming a task in `touched`, oldest first, round after round
    /// like a rescan of the whole queue: a group a landing makes ready joins this round when it
    /// comes later, the next one when it came earlier. A group still naming a task that is not
    /// here keeps waiting whole; a group that is complete applies, and what of it still fails
    /// waits alone at its place if it can, else goes to `out.skipped`.
    fn retry(&mut self, state: &mut DocState, touched: HashSet<TaskId>, out: &mut Parking) {
        let mut round = self.naming(&touched);
        // Bounded: a group leaves the queue or stays put; each round follows a landing.
        while !round.is_empty() {
            let mut later = BTreeSet::new();
            let mut pos = 0;
            while let Some(n) = round.range(pos..).next().copied() {
                let group = self.group_of(n);
                pos = group.last().map_or(n, |last| last + 1);
                let touched = self.retry_group(state, &group, out);
                let ready = self.naming(&touched);
                round.extend(ready.range(pos..));
                later.extend(ready.range(..pos));
            }
            round = later;
        }
    }

    /// One waiting group: back in the queue at its numbers unless it applies; the tasks its
    /// landed ops touched.
    fn retry_group(
        &mut self,
        state: &mut DocState,
        group: &[u64],
        out: &mut Parking,
    ) -> HashSet<TaskId> {
        let ops: Vec<(u64, Op)> = group
            .iter()
            .filter_map(|n| self.ops.get(n).map(|(op, _)| (*n, op.clone())))
            .collect();
        let bare: Vec<Op> = ops.iter().map(|(_, op)| op.clone()).collect();
        if needs_a_missing_task(state, &bare) {
            return HashSet::new();
        }
        for n in group {
            self.take(*n);
        }
        let failed = apply_leniently(state, &bare);
        let landed: Vec<&Op> = bare
            .iter()
            .filter(|op| !failed.iter().any(|(f, _)| f.id == op.id))
            .collect();
        out.landed += landed.len();
        landed.iter().for_each(|op| log_unparked(op));
        for (op, e) in failed {
            match ops.iter().find(|(_, o)| o.id == op.id) {
                Some((n, _)) if waits(state, &e) => self.put(*n, op.clone(), e),
                _ => out.skipped.push((op.clone(), e)),
            }
        }
        landed
            .into_iter()
            .filter_map(|op| subject(&op.kind))
            .collect()
    }
}

/// Applies `ops` one commit group at a time like `apply_leniently`. A group naming a task with no
/// placement here waits whole in `parked`; an op that fails because a task is not here or its text
/// edit does not fit yet waits alone. After each group that applied something, the waiting groups
/// naming a task it touched are retried. Anything else that does not fit, and the oldest waiting
/// op past the bound, is skipped.
pub(crate) fn apply_parking(state: &mut DocState, ops: &[Op], parked: &mut Parked) -> Parking {
    let mut out = Parking::default();
    for group in ops.chunk_by(|a, b| a.hlc == b.hlc) {
        if needs_a_missing_task(state, group) {
            for op in group {
                let missing = StateError::UnknownTask(first_name(op));
                parked.push(op.clone(), missing, &mut out.skipped);
            }
            continue;
        }
        let failed = apply_leniently(state, group);
        let touched: HashSet<TaskId> = group
            .iter()
            .filter(|op| !failed.iter().any(|(f, _)| f.id == op.id))
            .filter_map(|op| subject(&op.kind))
            .collect();
        for (op, e) in failed {
            if waits(state, &e) {
                parked.push(op.clone(), e, &mut out.skipped);
            } else {
                out.skipped.push((op.clone(), e));
            }
        }
        if !touched.is_empty() {
            parked.retry(state, touched, &mut out);
        }
    }
    debug_assert!(parked.ops.len() <= MAX_PARKED_OPS);
    out
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
        let [subject, anchor] = names(op);
        subject.as_ref().is_some_and(missing) || anchor.as_ref().is_some_and(missing)
    })
}

/// The task an op acts on (none for an insert, which brings its own) and the one it is placed
/// after in this document (a move to another file is placed there, not here).
fn names(op: &Op) -> [Option<TaskId>; 2] {
    match &op.kind {
        OpKind::Insert { after, .. } => [None, *after],
        OpKind::Move {
            task,
            after,
            to_file,
        } => [Some(*task), after.filter(|_| *to_file == op.file)],
        OpKind::SetField { task, .. }
        | OpKind::EditText { task, .. }
        | OpKind::RemoveTag { task, .. } => [Some(*task), None],
        OpKind::BlankInsert { after } | OpKind::BlankRemove { after } => [None, *after],
        OpKind::NotesEdit { .. } => [None, None],
    }
}

/// The task an op places or changes, insert included: what a waiting op naming it waits for.
fn subject(kind: &OpKind) -> Option<TaskId> {
    match kind {
        OpKind::Insert { task, .. }
        | OpKind::SetField { task, .. }
        | OpKind::EditText { task, .. }
        | OpKind::RemoveTag { task, .. }
        | OpKind::Move { task, .. } => Some(*task),
        OpKind::NotesEdit { .. } | OpKind::BlankInsert { .. } | OpKind::BlankRemove { .. } => None,
    }
}

/// For a waiting op's recorded reason: the first task it names.
fn first_name(op: &Op) -> TaskId {
    let [subject, anchor] = names(op);
    subject
        .or(anchor)
        .unwrap_or(TaskId::new(txtodo_model::Ulid::from_u128(0)))
}

/// Whether an op that failed with `e` may fit later: it names a task with no placement here,
/// shown or ghost (one that has not arrived yet; a deleted task keeps a ghost, so an op on it is
/// not this), or its text edit does not fit the line as this device has it yet.
fn waits(state: &DocState, e: &StateError) -> bool {
    match e {
        StateError::UnknownTask(id) => !state.has_placement(*id),
        StateError::Text(..) => true,
        _ => false,
    }
}

/// <https://docs.rs/tracing/latest/tracing/macro.debug.html>
fn log_parked(op: &Op) {
    tracing::debug!(file = %op.file, op = %op.id.ulid(), "sync_op_parked");
}

fn log_unparked(op: &Op) {
    tracing::debug!(file = %op.file, op = %op.id.ulid(), "sync_op_unparked");
}

/// Warn: an op leaves the queue for good, past the bound, not because it cannot fit.
fn log_overflow(op: &Op) {
    tracing::warn!(file = %op.file, op = %op.id.ulid(), max = MAX_PARKED_OPS, "sync_parked_overflow");
}
