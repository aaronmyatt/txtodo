//! The two ops that rewrite a task line in place: `SetField` (prefix fields, delete) and `EditText`
//! (description). Split from `state.rs` so each module stays under the file budget.

use crate::state::{DocState, Entry, StateError};
use txtodo_core::{Edit, LineKind, OwnedLine, Prefix, Priority, description_start, emit_prefix};
use txtodo_model::{Field, FieldValue, Hlc, IdentityMode, TaskId, TextEdit};

/// Applies `SetField` stamped `hlc`. `Deleted = true` removes the entry; other fields re-emit the
/// prefix, unless the field already took a newer stamp: then the op is older than what is here
/// and loses on every device, whatever order they got the two in (task partition-converge).
pub(crate) fn set_field(
    state: &mut DocState,
    task: TaskId,
    (field, value): (Field, FieldValue),
    hlc: Hlc,
) -> Result<(), StateError> {
    if field == Field::Deleted {
        return match value {
            // The line stays as a ghost where it was (ADR 0033), so an op a peer anchored on it
            // still lands; deleting it again, or after it moved away, changes nothing.
            FieldValue::Bool(true) => match state.live_slot(task) {
                Some(slot) => {
                    state.hide_entry(slot);
                    state.forget_fields(task);
                    Ok(())
                }
                None if state.has_placement(task) => Ok(()),
                None => Err(StateError::UnknownTask(task)),
            },
            _ => Err(StateError::Unsupported("undelete via SetField")),
        };
    }
    let Some(i) = live_or_gone(state, task)? else {
        return Ok(());
    };
    if state.is_stale_field(task, field, hlc) {
        log_stale_field(task, field);
        return Ok(());
    }
    let line = state.line_of(task).ok_or(StateError::UnknownTask(task))?;
    let new_line = rewrite_prefix(&line, field, value)
        .ok_or(StateError::Unsupported("SetField on this line"))?;
    debug_assert!(
        state.mode() != IdentityMode::Tagged || crate::state::id_of(&new_line) == Some(task),
        "prefix rewrite keeps the id"
    );
    if description_of(&line) != description_of(&new_line) {
        // A priority moved into `pri:`: the kept edits no longer rebuild this description.
        state.forget_text(task);
    }
    state.replace_entry(
        i,
        Entry::Task {
            id: task,
            line: new_line,
        },
    );
    state.record_field(task, field, hlc);
    Ok(())
}

/// A peer's `SetField` lost to a newer one on the same field: expected when two devices change one
/// field at once, so debug. <https://docs.rs/tracing/latest/tracing/macro.debug.html>
fn log_stale_field(task: TaskId, field: Field) {
    tracing::debug!(%task, ?field, "set_field_older_than_field");
}

/// Applies `EditText` stamped `hlc` to the description; the prefix bytes are spliced from the
/// original. A late edit rebuilds the description in stamp order (ADR 0034, `text_history.rs`).
/// On `Err` the state, its history included, is unchanged.
pub(crate) fn edit_text(
    state: &mut DocState,
    task: TaskId,
    edits: &[TextEdit],
    hlc: Hlc,
) -> Result<(), StateError> {
    let Some(i) = live_or_gone(state, task)? else {
        return Ok(());
    };
    let line = state.line_of(task).ok_or(StateError::UnknownTask(task))?;
    let description = description_of(&line).ok_or(StateError::Opaque(i))?;
    let saved = state.text_history_of(task);
    let new_line = state
        .edit_description(task, &description, hlc, edits)
        .map_err(|e| StateError::Text(task, e))
        .and_then(|new_description| {
            let edit = Edit::new()
                .set_description(&new_description)
                .map_err(|_| StateError::Unsupported("line break"))?;
            let new_line = txtodo_core::apply(&line, &edit);
            if state.mode() == IdentityMode::Tagged && crate::state::id_of(&new_line) != Some(task)
            {
                return Err(StateError::IdMismatch(task));
            }
            Ok(new_line)
        })
        .inspect_err(|_| state.restore_text(task, saved))?;
    state.replace_entry(
        i,
        Entry::Task {
            id: task,
            line: new_line,
        },
    );
    Ok(())
}

/// The slot of `task`'s shown line; `None` when it is only a ghost (deleted, or moved to another
/// file): a peer edited it before it saw that, and the edit has no line left to change, on any
/// device (lab lan-converge seed 435090918). Debug, like a stale field.
/// <https://docs.rs/tracing/latest/tracing/macro.debug.html>
fn live_or_gone(state: &DocState, task: TaskId) -> Result<Option<usize>, StateError> {
    match state.live_slot(task) {
        Some(i) => Ok(Some(i)),
        None if state.has_placement(task) => {
            tracing::debug!(%task, "edit_of_a_gone_task");
            Ok(None)
        }
        None => Err(StateError::UnknownTask(task)),
    }
}

/// A task line's description, `None` for anything else.
fn description_of(line: &OwnedLine) -> Option<String> {
    match line.parse()?.kind {
        LineKind::Task(t) => Some(t.description.to_owned()),
        LineKind::Blank => None,
    }
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
        (Field::Completed, FieldValue::Bool(b)) => {
            prefix.completed = b;
            // `x <date> text` reads that date as the completion date, so a creation date with
            // no completion date beside it would come back as one and the creation date would be
            // lost (it was, on every `Completed` then `CompletionDate` pair `change_ops` sends).
            // Keep it where it is by repeating it as the completion date until that field's own
            // op, next in the same batch, sets the real one. todo.txt format:
            // https://github.com/todotxt/todo.txt#todotxt-format-rules
            if b && prefix.completion_date.is_none() && prefix.creation_date.is_some() {
                prefix.completion_date = prefix.creation_date;
            }
        }
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
