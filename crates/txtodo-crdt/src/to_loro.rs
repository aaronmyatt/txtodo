//! `OpKind -> LoroDocument` mutations, exhaustive over the closed `OpKind` set.
//!
//! One op is one Loro commit: mutations are applied immediately, then `commit()` groups them into
//! one observable diff. A cross-file `Move` touches two lists inside that single commit so a peer
//! never observes the task in neither list (Loro has no cross-container transaction; the commit is
//! the atomic batch boundary). Loro list API: <https://loro.dev/docs/tutorial/list>.

use std::fmt;

use loro::LoroMovableList;
use txtodo_core::{LineKind, Mode, Ulid, parse_line};
use txtodo_model::{Field, FieldValue, FilePath, Hlc, Op, OpKind, TaskId, TextEdit};

use crate::doc::{
    DESCRIPTION_KEY, LoroDocument, encode_field_value, field_key, index_of, is_blank, task_id_str,
};
use crate::lww::write_if_newer;

/// Why an op could not be applied to the Loro document.
#[derive(Debug)]
pub enum ToLoroError {
    /// The op kind is not represented in this document (notes edits are M5).
    Unsupported(&'static str),
    /// A predecessor or moved task was not found in the list.
    TaskNotFound(TaskId),
    /// `BlankRemove` found no blank sentinel after the anchor (deleted tasks are skipped).
    NoBlankAfter(Option<TaskId>),
    /// An `Insert` line did not parse as a task.
    NotATask(String),
    /// The underlying Loro operation failed.
    Loro(loro::LoroError),
}

impl fmt::Display for ToLoroError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ToLoroError::Unsupported(what) => write!(f, "unsupported in this document: {what}"),
            ToLoroError::TaskNotFound(t) => write!(f, "no task {t} in the list"),
            ToLoroError::NoBlankAfter(a) => write!(f, "no blank line after {a:?}"),
            ToLoroError::NotATask(line) => write!(f, "insert line is not a task: {line:?}"),
            ToLoroError::Loro(e) => write!(f, "loro: {e}"),
        }
    }
}

impl std::error::Error for ToLoroError {}

impl From<loro::LoroError> for ToLoroError {
    fn from(e: loro::LoroError) -> ToLoroError {
        ToLoroError::Loro(e)
    }
}

/// Applies one op to the document, then commits once so the op is a single diff batch.
pub fn apply(doc: &mut LoroDocument, op: &Op) -> Result<(), ToLoroError> {
    match &op.kind {
        OpKind::Insert { task, after, line } => insert(doc, op, *task, *after, line)?,
        OpKind::SetField { task, field, value } => set_field(doc, op, *task, *field, *value)?,
        OpKind::EditText { task, edits } => edit_text(doc, *task, edits)?,
        OpKind::Move {
            task,
            after,
            to_file,
        } => mov(doc, op, *task, *after, to_file)?,
        OpKind::NotesEdit { .. } => {
            return Err(ToLoroError::Unsupported(
                "notes edits live in a separate M5 text document",
            ));
        }
        OpKind::BlankInsert { after } => blank_insert(doc, op, *after)?,
        OpKind::BlankRemove { after } => blank_remove(doc, op, *after)?,
    }
    doc.commit();
    Ok(())
}

/// Inserts the task into the file list and populates its task map from the parsed line. An id
/// already in the list (a deleted task being re-inserted by undo) is moved, not duplicated, and
/// its description is reset — one id, one list entry, always.
fn insert(
    doc: &mut LoroDocument,
    op: &Op,
    task: TaskId,
    after: Option<TaskId>,
    line: &str,
) -> Result<(), ToLoroError> {
    let list = doc.file_list(&op.file);
    if let Some(old) = index_of(&list, task) {
        list.delete(old, 1)?;
    }
    let idx = insert_index(&list, after)?;
    list.insert(idx, task_id_str(task))?;
    debug_assert!(index_of(&list, task) == Some(idx), "one entry per id");
    populate(doc, task, line, op.hlc)
}

/// Writes the description text and every prefix field of a freshly inserted task.
pub(crate) fn populate(
    doc: &LoroDocument,
    task: TaskId,
    line: &str,
    hlc: Hlc,
) -> Result<(), ToLoroError> {
    let parsed =
        parse_line(line, Mode::Lenient).map_err(|_| ToLoroError::NotATask(line.to_owned()))?;
    let LineKind::Task(t) = parsed.kind else {
        return Err(ToLoroError::NotATask(line.to_owned()));
    };
    let map = doc.task_map(task)?;
    let text = map.ensure_mergeable_text(DESCRIPTION_KEY)?;
    let stale = text.len_unicode();
    if stale > 0 {
        text.delete(0, stale)?;
    }
    text.insert(0, t.description)?;
    debug_assert_eq!(
        text.to_string(),
        t.description,
        "description is exactly the line's"
    );
    let fields = [
        (Field::Completed, FieldValue::Bool(t.completed)),
        (
            Field::CompletionDate,
            FieldValue::Date(t.completion_date.map(|d| (d.year(), d.month(), d.day()))),
        ),
        (
            Field::CreationDate,
            FieldValue::Date(t.creation_date.map(|d| (d.year(), d.month(), d.day()))),
        ),
        (
            Field::Priority,
            FieldValue::Priority(t.priority.map(|p| p.as_char())),
        ),
        (Field::Deleted, FieldValue::Bool(false)),
        (Field::Quirks, FieldValue::quirks(parsed.quirks)),
    ];
    for (field, value) in fields {
        write_if_newer(&map, field_key(field), encode_field_value(value), hlc)?;
    }
    Ok(())
}

/// Writes one prefix field through the LWW register.
fn set_field(
    doc: &mut LoroDocument,
    op: &Op,
    task: TaskId,
    field: Field,
    value: FieldValue,
) -> Result<(), ToLoroError> {
    let map = doc.task_map(task)?;
    write_if_newer(&map, field_key(field), encode_field_value(value), op.hlc)?;
    Ok(())
}

/// Replays char-level edits onto the task's description `LoroText`.
fn edit_text(doc: &mut LoroDocument, task: TaskId, edits: &[TextEdit]) -> Result<(), ToLoroError> {
    let text = doc.description_text(task)?;
    for edit in edits {
        match edit {
            TextEdit::Insert { at, text: s } => text.insert(*at, s.as_str())?,
            TextEdit::Delete { at, len } => text.delete(*at, *len)?,
        }
    }
    Ok(())
}

/// Moves a task: same file through `mov` (identity preserved), cross-file through delete + insert
/// in one commit.
fn mov(
    doc: &mut LoroDocument,
    op: &Op,
    task: TaskId,
    after: Option<TaskId>,
    to_file: &FilePath,
) -> Result<(), ToLoroError> {
    let source = doc.file_list(&op.file);
    let from = index_of(&source, task).ok_or(ToLoroError::TaskNotFound(task))?;
    if to_file == &op.file {
        let to = mov_to(&source, from, after)?;
        source.mov(from, to)?;
    } else {
        let target = doc.file_list(to_file);
        let to = insert_index(&target, after)?;
        source.delete(from, 1)?;
        target.insert(to, task_id_str(task))?;
    }
    Ok(())
}

/// Inserts a blank sentinel after `after`.
fn blank_insert(doc: &mut LoroDocument, op: &Op, after: Option<TaskId>) -> Result<(), ToLoroError> {
    let list = doc.file_list(&op.file);
    let idx = insert_index(&list, after)?;
    let sentinel = doc.blank_id();
    list.insert(idx, task_id_str(sentinel))?;
    Ok(())
}

/// Removes the first blank sentinel after `after`. Deleted tasks stay in the list as tombstones
/// and are skipped; a live task before any blank means there is no blank to remove.
fn blank_remove(doc: &mut LoroDocument, op: &Op, after: Option<TaskId>) -> Result<(), ToLoroError> {
    let list = doc.file_list(&op.file);
    let start = insert_index(&list, after)?;
    let idx = next_blank_index(doc, &list, start).ok_or(ToLoroError::NoBlankAfter(after))?;
    debug_assert!(idx >= start && idx < list.len());
    list.delete(idx, 1)?;
    Ok(())
}

/// The first blank sentinel at or after `start`, looking past deleted tasks only.
fn next_blank_index(doc: &LoroDocument, list: &LoroMovableList, start: usize) -> Option<usize> {
    let len = list.len();
    debug_assert!(start <= len);
    // Bounded by the list length.
    for idx in start..len {
        let id = id_at(list, idx)?;
        if is_blank(id) {
            return Some(idx);
        }
        if !doc.is_deleted(id) {
            return None;
        }
    }
    None
}

/// The insertion index for `after`: `0` for `None`, else the predecessor index plus one.
fn insert_index(list: &LoroMovableList, after: Option<TaskId>) -> Result<usize, ToLoroError> {
    match after {
        None => Ok(0),
        Some(a) => index_of(list, a)
            .map(|i| i + 1)
            .ok_or(ToLoroError::TaskNotFound(a)),
    }
}

/// The `to` index for a same-file `mov`, as the final index after the predecessor.
fn mov_to(
    list: &LoroMovableList,
    from: usize,
    after: Option<TaskId>,
) -> Result<usize, ToLoroError> {
    let Some(a) = after else {
        return Ok(0);
    };
    let a_idx = index_of(list, a).ok_or(ToLoroError::TaskNotFound(a))?;
    Ok(if a_idx < from { a_idx + 1 } else { a_idx })
}

/// The task id stored at list index `idx`, if that entry is one.
fn id_at(list: &LoroMovableList, idx: usize) -> Option<TaskId> {
    list.get(idx)
        .and_then(|voc| voc.into_value().ok())
        .and_then(|v| v.as_string().cloned())
        .and_then(|s| Ulid::parse(s.as_ref()))
        .map(TaskId::new)
}
