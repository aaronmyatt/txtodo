//! The two ops that rewrite a task line in place: `SetField` (prefix fields, delete) and `EditText`
//! (description). Split from `state.rs` so each module stays under the file budget.

use crate::state::{DocState, Entry, StateError};
use crate::textedit::apply_text_edits;
use txtodo_core::{Edit, LineKind, OwnedLine, Prefix, Priority, description_start, emit_prefix};
use txtodo_model::{Field, FieldValue, IdentityMode, TaskId, TextEdit};

/// Applies `SetField`. `Deleted = true` removes the entry; other fields re-emit the prefix.
pub(crate) fn set_field(
    state: &mut DocState,
    task: TaskId,
    field: Field,
    value: FieldValue,
) -> Result<(), StateError> {
    let i = state.index_of(task).ok_or(StateError::UnknownTask(task))?;
    if field == Field::Deleted {
        return match value {
            FieldValue::Bool(true) => {
                state.remove_entry(i);
                Ok(())
            }
            _ => Err(StateError::Unsupported("undelete via SetField")),
        };
    }
    let line = state.line_of(task).ok_or(StateError::UnknownTask(task))?;
    let new_line = rewrite_prefix(&line, field, value)
        .ok_or(StateError::Unsupported("SetField on this line"))?;
    debug_assert!(
        state.mode() != IdentityMode::Tagged || crate::state::id_of(&new_line) == Some(task),
        "prefix rewrite keeps the id"
    );
    state.replace_entry(
        i,
        Entry::Task {
            id: task,
            line: new_line,
        },
    );
    Ok(())
}

/// Applies `EditText` to the description; the prefix bytes are spliced from the original.
pub(crate) fn edit_text(
    state: &mut DocState,
    task: TaskId,
    edits: &[TextEdit],
) -> Result<(), StateError> {
    let i = state.index_of(task).ok_or(StateError::UnknownTask(task))?;
    let line = state.line_of(task).ok_or(StateError::UnknownTask(task))?;
    let description = match line.parse().map(|l| l.kind) {
        Some(LineKind::Task(t)) => t.description.to_owned(),
        _ => return Err(StateError::Opaque(i)),
    };
    let new_description =
        apply_text_edits(&description, edits).map_err(|e| StateError::Text(task, e))?;
    let edit = Edit::new()
        .set_description(&new_description)
        .map_err(|_| StateError::Unsupported("line break"))?;
    let new_line = txtodo_core::apply(&line, &edit);
    if state.mode() == IdentityMode::Tagged && crate::state::id_of(&new_line) != Some(task) {
        return Err(StateError::IdMismatch(task));
    }
    state.replace_entry(
        i,
        Entry::Task {
            id: task,
            line: new_line,
        },
    );
    Ok(())
}

/// Re-emits the prefix with one field changed; a priority on a completed line moves to `pri:`
/// (core `Edit::complete` rule). `None` when the line is not a task or the value does not fit.
fn rewrite_prefix(line: &OwnedLine, field: Field, value: FieldValue) -> Option<OwnedLine> {
    let raw = line.raw()?;
    let LineKind::Task(task) = line.parse()?.kind else {
        return None;
    };
    let mut prefix = Prefix::of(&task);
    match (field, value) {
        (Field::Completed, FieldValue::Bool(b)) => prefix.completed = b,
        (Field::CompletionDate, v) => prefix.completion_date = v.as_date()?,
        (Field::CreationDate, v) => prefix.creation_date = v.as_date()?,
        (Field::Priority, FieldValue::Priority(p)) => prefix.priority = p.and_then(Priority::new),
        (Field::Quirks, _) | (Field::Deleted, _) | (Field::Completed, _) | (Field::Priority, _) => {
            return None;
        }
    }
    let split = description_start(raw);
    let description = raw[split..].to_owned();
    let moved_priority = if prefix.completed {
        prefix.priority.take()
    } else {
        None
    };
    let text = emit_prefix(&prefix, !description.is_empty()) + &description;
    debug_assert!(!text.contains('\n'), "prefix rewrite keeps one line");
    let rewritten = OwnedLine::from_bytes(text.into_bytes(), line.ending());
    match moved_priority {
        Some(p) => {
            let edit = Edit::new()
                .set_tag("pri", p.as_char().encode_utf8(&mut [0; 4]))
                .ok()?;
            Some(txtodo_core::apply(&rewritten, &edit))
        }
        None => Some(rewritten),
    }
}
