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
            // still lands; deleting it again, or after it moved away, changes nothing. A delete
            // older than an insert again of the task (`state_reinsert.rs`) leaves it shown.
            // Its field stamps and history stay: a later insert again merges with them.
            FieldValue::Bool(true) if state.has_placement(task) => {
                let hides = state.note_death(task, hlc);
                if let Some(slot) = state.live_slot(task).filter(|_| hides) {
                    state.hide_entry(slot);
                }
                Ok(())
            }
            FieldValue::Bool(true) => Err(StateError::UnknownTask(task)),
            _ => Err(StateError::Unsupported("undelete via SetField")),
        };
    }
    // A deleted task still takes it, hidden (`state_reinsert.rs`).
    let i = state
        .content_slot(task)
        .ok_or(StateError::UnknownTask(task))?;
    let line = state.slot_line(i).clone();
    if state.is_stale_field(task, field, hlc) {
        log_stale_field(task, field);
        if field == Field::Priority
            && let Some(restored) = priority_from_tag(&line)
        {
            state.replace_entry(
                i,
                Entry::Task {
                    id: task,
                    line: restored,
                },
            );
        }
        return Ok(());
    }
    let new_line = rewrite_prefix(&line, field, value)
        .ok_or(StateError::Unsupported("SetField on this line"))?;
    debug_assert!(
        state.mode() != IdentityMode::Tagged || crate::state::id_of(&new_line) == Some(task),
        "prefix rewrite keeps the id"
    );
    let new_line = if description_of(&line) == description_of(&new_line) {
        new_line
    } else {
        keep_in_history(state, task, &line, new_line, hlc)
    };
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

/// A prefix rewrite changed the description: a priority moved into `pri:` on a done line, added
/// at the end or swapped in place. That goes into the description's history at `hlc` (ADR 0034,
/// amended 2026-10-02), so a text edit made apart and arriving late is slotted in before it on
/// every device, and the line it returns is the one the history rebuilds. A tag swapped in place
/// sets the letter of every tag the history holds instead (`TextHistory::swap_pri`): only the
/// newest priority gets here. A change the history's own `set_pri` would not make the same way,
/// or a swap it cannot make, drops the history, as before.
fn keep_in_history(
    state: &mut DocState,
    task: TaskId,
    old_line: &OwnedLine,
    new_line: OwnedLine,
    hlc: Hlc,
) -> OwnedLine {
    let kept = moved_into_pri(old_line, &new_line).and_then(|(old, new, pri)| {
        let held = state.set_pri_in_description(task, &old, hlc, pri)?;
        Some((held, new))
    });
    let Some((held, new)) = kept else {
        state.forget_text(task);
        return new_line;
    };
    if held == new {
        return new_line;
    }
    match Edit::new().set_description(&held) {
        Ok(edit) => txtodo_core::apply(&new_line, &edit),
        Err(_) => new_line,
    }
}

/// The old and new descriptions and `(letter, added)` when the rewrite moved a priority into
/// `pri:` exactly as `text_history::set_pri` would: added when the old text had no tag, else
/// swapped. `None` for any other change.
fn moved_into_pri(
    old_line: &OwnedLine,
    new_line: &OwnedLine,
) -> Option<(String, String, (char, bool))> {
    let (old, new) = (description_of(old_line)?, description_of(new_line)?);
    let letter = moved_priority(new_line)?;
    let add = !crate::text_history::has_pri(&old);
    (crate::text_history::set_pri(&old, letter, add) == new).then_some((old, new, (letter, add)))
}

/// The letter a done line's `pri:` tag holds, `None` on an open line or without one.
fn moved_priority(line: &OwnedLine) -> Option<char> {
    let LineKind::Task(task) = line.parse()?.kind else {
        return None;
    };
    if !Prefix::of(&task).completed {
        return None;
    }
    task.tag("pri")?.chars().next()
}

/// A reopen sends `Completed = false`, then the priority it restores, then a text edit dropping the
/// `pri:` tag. When a newer priority change landed on the done line first, that `Priority` op
/// loses, and the newer value is the one `pri:` holds: it goes into the prefix instead, so both
/// arrival orders end with the newer priority (task partition-converge). Only an open line with
/// a `pri:` tag and no `(X)` takes it: a reopen half applied, or a tag a user typed. `None` for
/// any other line, which a lost op leaves as it is.
fn priority_from_tag(line: &OwnedLine) -> Option<OwnedLine> {
    let LineKind::Task(task) = line.parse()?.kind else {
        return None;
    };
    let prefix = Prefix::of(&task);
    if prefix.completed || prefix.priority.is_some() {
        return None;
    }
    let letter = task.tag("pri")?.chars().next()?;
    Priority::new(letter)?;
    rewrite_prefix(line, Field::Priority, FieldValue::Priority(Some(letter)))
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
    let i = state
        .content_slot(task)
        .ok_or(StateError::UnknownTask(task))?;
    let line = state.slot_line(i).clone();
    let description = description_of(&line).ok_or(StateError::Opaque(i))?;
    let saved = state.text_history_of(task);
    let new_line = state
        .edit_description(task, &description, hlc, edits)
        .map_err(|e| StateError::Text(task, e))
        .and_then(|new_description| with_description(state, task, &line, &new_description))
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

/// Applies `RemoveTag` stamped `hlc` (ADR 0036): the first `key:` tag of the description goes,
/// wherever a late edit slotted in front of it put the tag. Kept in the description's history
/// like a text edit. A bad key, or an `id:` the tagged line needs, is refused, the state unchanged.
pub(crate) fn remove_tag(
    state: &mut DocState,
    task: TaskId,
    key: &str,
    hlc: Hlc,
) -> Result<(), StateError> {
    if !txtodo_model::valid_tag_key(key) {
        return Err(StateError::Unsupported("RemoveTag with a bad key"));
    }
    let i = state
        .content_slot(task)
        .ok_or(StateError::UnknownTask(task))?;
    let line = state.slot_line(i).clone();
    let description = description_of(&line).ok_or(StateError::Opaque(i))?;
    let saved = state.text_history_of(task);
    let held = state.remove_tag_in_description(task, &description, hlc, key);
    let new_line = with_description(state, task, &line, &held)
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

/// `line` with its description set to `text`; refused when that drops the `id:` a tagged line
/// needs.
fn with_description(
    state: &DocState,
    task: TaskId,
    line: &OwnedLine,
    text: &str,
) -> Result<OwnedLine, StateError> {
    let edit = Edit::new()
        .set_description(text)
        .map_err(|_| StateError::Unsupported("line break"))?;
    let new_line = txtodo_core::apply(line, &edit);
    if state.mode() == IdentityMode::Tagged && crate::state::id_of(&new_line) != Some(task) {
        return Err(StateError::IdMismatch(task));
    }
    Ok(new_line)
}

/// A task line's description, `None` for anything else.
pub(crate) fn description_of(line: &OwnedLine) -> Option<String> {
    match line.parse()?.kind {
        LineKind::Task(t) => Some(t.description.to_owned()),
        LineKind::Blank => None,
    }
}

/// Re-emits the prefix with one field changed; a priority on a completed line moves to `pri:`
/// (core `Edit::complete` rule). `None` when the line is not a task or the value does not fit.
pub(crate) fn rewrite_prefix(
    line: &OwnedLine,
    field: Field,
    value: FieldValue,
) -> Option<OwnedLine> {
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

/// `field`'s value on a task line, for the four prefix fields; `None` for anything else.
pub(crate) fn field_value(line: &OwnedLine, field: Field) -> Option<FieldValue> {
    let LineKind::Task(task) = line.parse()?.kind else {
        return None;
    };
    let prefix = Prefix::of(&task);
    match field {
        Field::Completed => Some(FieldValue::Bool(prefix.completed)),
        Field::CompletionDate => Some(FieldValue::date(prefix.completion_date)),
        Field::CreationDate => Some(FieldValue::date(prefix.creation_date)),
        Field::Priority => Some(FieldValue::priority(prefix.priority)),
        Field::Deleted | Field::Quirks => None,
    }
}
