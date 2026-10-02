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

use std::collections::VecDeque;

use crate::state::{DocState, StateError};
use crate::sync_ops::apply_leniently;
use txtodo_model::Op;

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
    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.ops.len()
    }

    /// Retries every waiting op until a round lands none; returns how many landed.
    fn retry(&mut self, state: &mut DocState) -> usize {
        let mut landed = 0;
        // Bounded: every round but the last lands at least one op.
        loop {
            let before = self.ops.len();
            self.ops.retain_mut(|(op, e)| match state.apply(op) {
                Ok(()) => {
                    log_unparked(op);
                    landed += 1;
                    false
                }
                Err(err) => {
                    *e = err;
                    true
                }
            });
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
