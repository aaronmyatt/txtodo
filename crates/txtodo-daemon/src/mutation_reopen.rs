//! `Mutation::Reopen` (task `complete-to-bottom`): the inverse of `Complete`. Clears the `x` and
//! the completion date and turns `pri:X` back into `(X)` (core `Edit::uncomplete`), then moves the
//! line to the end of the open block, just above the first done line, so open lines stay on top.
//! Its old place is not remembered. Split out of `mutation_moves.rs` for its file budget.

use crate::mutation::{MutationError, TaskRef, resolve};
use crate::reconcile::change_ops;
use crate::state::{DocState, is_completed};
use txtodo_core::Edit;
use txtodo_model::{Field, OpKind, TaskId};

/// The ops that reopen `task`. Empty when the line is not done: reopening an open task is not an
/// error and never moves it.
pub(crate) fn reopen_ops(state: &DocState, task: &TaskRef) -> Result<Vec<OpKind>, MutationError> {
    let (i, id) = resolve(state, task)?;
    let old = state
        .line_of(id)
        .ok_or(MutationError::NoLine(task.line_number))?;
    let new = txtodo_core::apply(&old, &Edit::new().uncomplete());
    let mut ops = change_ops(&old, &new, id);
    if ops.is_empty() {
        return Ok(ops);
    }
    // `change_ops` puts the priority first, which is right for completing (the priority must be
    // set while the line is open, or it would render as `pri:`). Reopening is the mirror: clear
    // the `x` first, so the restored `(B)` lands on an open line and is not folded back into a
    // `pri:B` tag that the description edit then deletes.
    ops.sort_by_key(|op| match op {
        OpKind::SetField {
            field: Field::Completed,
            ..
        } => 0,
        OpKind::SetField {
            field: Field::CompletionDate,
            ..
        } => 1,
        _ => 2,
    });
    let after = end_of_open_block(state, id);
    // Already right after its anchor: no move, so it adds no op.
    if state.task_before(i) != after {
        ops.push(OpKind::Move {
            task: id,
            after,
            to_file: state.path().clone(),
        });
    }
    Ok(ops)
}

/// The task the reopened line goes after: the one just before the first done line (leaving `id`
/// itself out of the count), or the last task when nothing else is done. `None` puts it first,
/// which is where it belongs when every other task in the file is done.
fn end_of_open_block(state: &DocState, id: TaskId) -> Option<TaskId> {
    let mut last_open: Option<TaskId> = None;
    for (other, line) in state.task_lines() {
        if other == id {
            continue;
        }
        if is_completed(line) {
            return last_open;
        }
        last_open = Some(other);
    }
    last_open
}
