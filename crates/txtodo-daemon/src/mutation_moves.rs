//! The move-shaped mutations' op generation and the read-only `peek_line`, split out of
//! `mutation.rs` for its file budget: the cross-file `Move` source half, `MoveToEnd` (archiving),
//! `MoveBefore` (task `mcp-move-reorder`) and the `PeekedLine` a cross-file move captures first.

use crate::mutation::{MutationError, TaskRef, resolve};
use crate::state::DocState;
use txtodo_core::LineKind;
use txtodo_model::{FilePath, OpKind, TaskId};

/// The source half of a cross-file `Move`: this document only ever records that the task left
/// (`OpKind::Move` with no in-file `after` — `to` is a different document, so no position here is
/// meaningful). `crate::move_coordinator` resolves the destination anchor and inserts the line
/// there as its own `Add`; see that module for the full, two-actor operation and the `ref:`
/// directory relocation that rides along with it.
pub(crate) fn move_ops(
    state: &DocState,
    task: &TaskRef,
    to: &FilePath,
) -> Result<Vec<OpKind>, MutationError> {
    let (_, id) = resolve(state, task)?;
    Ok(vec![OpKind::Move {
        task: id,
        after: None,
        to_file: to.clone(),
    }])
}

/// The archive half of `Mutation::MoveToEnd`: appends the task after whichever other task is
/// currently last, so archiving several tasks in original relative order lands them at the bottom
/// in that same order (`DocState::move_task`'s same-file branch does the actual reorder).
pub(crate) fn move_to_end_ops(
    state: &DocState,
    task: &TaskRef,
) -> Result<Vec<OpKind>, MutationError> {
    let (i, id) = resolve(state, task)?;
    let last = state.task_before(state.len());
    // Already last: anchor to its own predecessor instead, so the reorder is a true no-op.
    let after = if last == Some(id) {
        state.task_before(i)
    } else {
        last
    };
    Ok(vec![OpKind::Move {
        task: id,
        after,
        to_file: state.path().clone(),
    }])
}

/// `Mutation::MoveBefore`: anchors the task after whichever task currently precedes `before`
/// (`DocState::move_task`'s same-file branch does the reorder). When that predecessor is the task
/// itself it already sits right there, so it anchors to its own predecessor instead — a true
/// no-op, the same trick `move_to_end_ops` uses.
pub(crate) fn move_before_ops(
    state: &DocState,
    task: &TaskRef,
    before: &TaskRef,
) -> Result<Vec<OpKind>, MutationError> {
    let (i, id) = resolve(state, task)?;
    let (j, before_id) = resolve(state, before)?;
    if id == before_id {
        return Err(MutationError::Unsupported("moving a task before itself"));
    }
    let predecessor = state.task_before(j);
    let after = if predecessor == Some(id) {
        state.task_before(i)
    } else {
        predecessor
    };
    Ok(vec![OpKind::Move {
        task: id,
        after,
        to_file: state.path().clone(),
    }])
}

/// A task line read without mutating anything: its id, full raw bytes and `ref:` slug (if any).
/// `crate::move_coordinator` uses this to capture what a cross-file `Move` carries before it
/// touches either document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeekedLine {
    /// The task's id.
    pub id: TaskId,
    /// The line's exact bytes (no ending), `id:` tag included.
    pub line: String,
    /// Its `ref:` tag value, when it has one valid per plan §3.2.1.
    pub ref_slug: Option<String>,
}

/// Resolves `task` against `bytes` (a document's current projection, e.g. from `ActorHandle::get`)
/// without any actor round trip: read-only, so a caller may inspect a line before deciding what
/// mutation to send. `MutationError::Stale` on a mismatched id, exactly like [`resolve`].
pub fn peek_line(bytes: &[u8], task: &TaskRef) -> Result<PeekedLine, MutationError> {
    let n = task.line_number;
    let i = n.checked_sub(1).ok_or(MutationError::NoLine(n))?;
    let file = txtodo_core::parse_file(bytes);
    let line = file.lines.get(i).ok_or(MutationError::NoLine(n))?;
    let parsed = line.parse().ok_or(MutationError::NoLine(n))?;
    let LineKind::Task(t) = parsed.kind else {
        return Err(MutationError::Blank(n));
    };
    let found = t.id().map(TaskId::new).ok_or(MutationError::NoLine(n))?;
    if let Some(expected) = task.task_id
        && expected != found
    {
        return Err(MutationError::Stale {
            line_number: n,
            expected,
            found,
        });
    }
    Ok(PeekedLine {
        id: found,
        line: line.raw().unwrap_or_default().to_owned(),
        ref_slug: t.ref_slug().map(str::to_owned),
    })
}
