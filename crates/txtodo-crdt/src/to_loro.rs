//! `OpKind -> LoroDocument` mutations, exhaustive over the closed `OpKind` set.
//!
//! One op is one Loro commit: mutations are applied immediately, then `commit()` groups them into
//! one observable diff. A cross-file `Move` touches two lists inside that single commit so a peer
//! never observes the task in neither list (Loro has no cross-container transaction; the commit is
//! the atomic batch boundary). Every list index comes from the per-file shadow (`doc/shadow.rs`),
//! never from a Loro walk. Loro list API: <https://loro.dev/docs/tutorial/list>.

use std::fmt;

use loro::{LoroResult, LoroText};
use txtodo_core::{LineKind, Mode, parse_line};
use txtodo_model::{Field, FieldValue, FilePath, Hlc, Op, OpKind, TaskId, TextEdit};

use crate::doc::{DESCRIPTION_KEY, LoroDocument, encode_field_value, field_key, is_blank};
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

/// Applies one op to the document, then commits once so the op is a single diff batch. Thin
/// wrapper around `apply_inner` for the tracing span (`#[instrument]` on the real body overflows
/// the `cognitive_complexity` budget).
#[tracing::instrument(skip_all, fields(file = %op.file, kind = op_kind_name(&op.kind)))]
pub fn apply(doc: &mut LoroDocument, op: &Op) -> Result<(), ToLoroError> {
    apply_inner(doc, op)
}

fn apply_inner(doc: &mut LoroDocument, op: &Op) -> Result<(), ToLoroError> {
    doc.ensure_shadow(&op.file);
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

/// The op kind's name, for the tracing span — never the op's own payload (line text, edit spans,
/// field values).
fn op_kind_name(kind: &OpKind) -> &'static str {
    match kind {
        OpKind::Insert { .. } => "insert",
        OpKind::SetField { .. } => "set_field",
        OpKind::EditText { .. } => "edit_text",
        OpKind::Move { .. } => "move",
        OpKind::NotesEdit { .. } => "notes_edit",
        OpKind::BlankInsert { .. } => "blank_insert",
        OpKind::BlankRemove { .. } => "blank_remove",
    }
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
    if let Some(old) = doc.index_in(&op.file, task) {
        doc.list_delete(&op.file, old)?;
    }
    let idx = insert_index(doc, &op.file, after)?;
    doc.list_insert(&op.file, idx, task)?;
    debug_assert_eq!(doc.index_in(&op.file, task), Some(idx), "one entry per id");
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
    // The op carries the model's dual-index convention; `replay_edits` speaks the evolving one.
    let edits: Vec<txtodo_core::TextEdit> = edits.iter().cloned().map(Into::into).collect();
    replay_edits(&text, &edits)?;
    debug_assert_eq!(text.len_unicode(), text.to_string().chars().count());
    Ok(())
}

/// Replays a `diff_text`-convention edit stream onto a Loro text.
///
/// The stream is dual-indexed (`txtodo_model::TextEdit`): a `Delete`'s `at` counts the *source*,
/// an `Insert`'s `at` counts the *target*, both in order. Loro holds one evolving string, so each
/// position is translated through two cursors — an insert before a later delete shifts that delete.
/// A naive single-cursor replay lands on the wrong characters whenever both kinds appear.
/// Positions are Unicode code points, matching `LoroText::len_unicode`:
/// <https://docs.rs/loro/latest/loro/struct.LoroText.html#method.insert>
pub(crate) fn replay_edits(text: &LoroText, edits: &[txtodo_core::TextEdit]) -> LoroResult<()> {
    let mut cursor = 0usize; // next unconsumed char of the source text
    let mut emitted = 0usize; // chars of the target already produced or kept
    for edit in edits {
        match edit {
            txtodo_core::TextEdit::Delete { at, len } => {
                debug_assert!(
                    *at >= cursor,
                    "a delete never moves backwards in the source"
                );
                let pos = emitted + (at - cursor);
                text.delete(pos, *len)?;
                cursor = at + len;
                emitted = pos;
            }
            txtodo_core::TextEdit::Insert { at, text: s } => {
                debug_assert!(
                    *at >= emitted,
                    "an insert never moves backwards in the target"
                );
                let keep = at - emitted;
                let pos = emitted + keep;
                text.insert(pos, s)?;
                cursor += keep;
                emitted = pos + s.chars().count();
            }
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
    let from = doc
        .index_in(&op.file, task)
        .ok_or(ToLoroError::TaskNotFound(task))?;
    if to_file == &op.file {
        let to = mov_to(doc, &op.file, from, after)?;
        doc.list_mov(&op.file, from, to)?;
    } else {
        doc.ensure_shadow(to_file);
        let to = insert_index(doc, to_file, after)?;
        doc.list_delete(&op.file, from)?;
        doc.list_insert(to_file, to, task)?;
    }
    debug_assert!(doc.index_in(to_file, task).is_some());
    Ok(())
}

/// Inserts a blank sentinel after `after`.
fn blank_insert(doc: &mut LoroDocument, op: &Op, after: Option<TaskId>) -> Result<(), ToLoroError> {
    let idx = insert_index(doc, &op.file, after)?;
    let sentinel = doc.blank_id();
    doc.list_insert(&op.file, idx, sentinel)?;
    debug_assert_eq!(doc.id_at(&op.file, idx), Some(sentinel));
    Ok(())
}

/// Removes the first blank sentinel after `after`. Deleted tasks stay in the list as tombstones
/// and are skipped; a live task before any blank means there is no blank to remove.
fn blank_remove(doc: &mut LoroDocument, op: &Op, after: Option<TaskId>) -> Result<(), ToLoroError> {
    let start = insert_index(doc, &op.file, after)?;
    let idx = next_blank_index(doc, &op.file, start).ok_or(ToLoroError::NoBlankAfter(after))?;
    debug_assert!(idx >= start && idx < doc.len_of(&op.file));
    doc.list_delete(&op.file, idx)?;
    Ok(())
}

/// The first blank sentinel at or after `start`, looking past deleted tasks only.
fn next_blank_index(doc: &LoroDocument, file: &FilePath, start: usize) -> Option<usize> {
    let len = doc.len_of(file);
    debug_assert!(start <= len);
    // Bounded by the list length.
    for idx in start..len {
        let id = doc.id_at(file, idx)?;
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
fn insert_index(
    doc: &LoroDocument,
    file: &FilePath,
    after: Option<TaskId>,
) -> Result<usize, ToLoroError> {
    match after {
        None => Ok(0),
        Some(a) => doc
            .index_in(file, a)
            .map(|i| i + 1)
            .ok_or(ToLoroError::TaskNotFound(a)),
    }
}

/// The `to` index for a same-file `mov`, as the final index after the predecessor.
fn mov_to(
    doc: &LoroDocument,
    file: &FilePath,
    from: usize,
    after: Option<TaskId>,
) -> Result<usize, ToLoroError> {
    let Some(a) = after else {
        return Ok(0);
    };
    let a_idx = doc.index_in(file, a).ok_or(ToLoroError::TaskNotFound(a))?;
    debug_assert!(a_idx != from, "a task is not its own anchor");
    Ok(if a_idx < from { a_idx + 1 } else { a_idx })
}
