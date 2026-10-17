//! `Add` and `Arrive` as ops, and the client-text check every line-writing mutation shares; split
//! out of `mutation.rs` for its file budget.

use crate::mutation::MutationError;
use crate::state::{DocState, id_of};
use txtodo_core::{Edit, LineKind, OwnedLine};
use txtodo_model::{IdentityMode, OpKind, TaskId};

/// One `Insert` at the end of the open block. `mint` supplies the id: a fresh one for an `Add`
/// (Sidecar, or Tagged with no `id:` in the text, which then gains the tag), the task's own for an
/// `Arrive`.
pub(crate) fn add_ops(
    state: &DocState,
    line: &str,
    mint: &mut dyn FnMut() -> TaskId,
) -> Result<Vec<OpKind>, MutationError> {
    let owned = task_line(line)?;
    let (task, owned) = if state.mode() == IdentityMode::Sidecar {
        (mint(), owned)
    } else {
        match id_of(&owned) {
            Some(id) => (id, owned),
            None => {
                let id = mint();
                let tag = id.ulid().to_string();
                let edit = Edit::new()
                    .set_tag("id", &tag)
                    .map_err(|_| MutationError::NotATask(line.to_owned()))?;
                (id, txtodo_core::apply(&owned, &edit))
            }
        }
    };
    let after = state.task_before(state.len());
    let text = owned.raw().unwrap_or_default().to_owned();
    debug_assert!(
        state.mode() != IdentityMode::Tagged || text.contains("id:"),
        "an added line always carries its id in tagged mode"
    );
    Ok(vec![OpKind::Insert {
        task,
        after,
        line: text,
    }])
}

/// Validates client text into a task line (LF ending; the state re-ends it on insert).
pub(crate) fn task_line(text: &str) -> Result<OwnedLine, MutationError> {
    if text.contains('\n') || text.contains('\r') {
        return Err(MutationError::NotATask(text.to_owned()));
    }
    let owned = OwnedLine::from_bytes(text.as_bytes().to_vec(), txtodo_core::LineEnding::default());
    match owned.parse().map(|l| l.kind) {
        Some(LineKind::Task(_)) => Ok(owned),
        _ => Err(MutationError::NotATask(text.to_owned())),
    }
}
