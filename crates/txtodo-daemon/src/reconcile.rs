//! File → ops (design §4.3 steps 3–6, one device): the bytes just read against the bytes we last
//! wrote, keyed by `id:`, become ops the actor applies to its state. Pure: no I/O, no clock — the
//! caller supplies a minting closure for fresh ids. The actor checks the postcondition
//! `apply(ops) == file` and adopts the file if a quirky line defeats the op model.
//!
//! Four passes: deletes (bottom-up), same-id changes, task inserts and moves (top-down, each right
//! after its predecessor task), then new blank lines anchored to the task above them.

use crate::state::id_of;
use std::collections::{BTreeMap, VecDeque};
use txtodo_core::{Edit, File, LineDiff, LineKind, OwnedLine, Task, diff_lines, diff_text};
use txtodo_model::{Field, FieldValue, FilePath, OpKind, TaskId, TextEdit, set_field};

/// The result of one reconcile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reconciled {
    /// Ops in application order.
    pub ops: Vec<OpKind>,
    /// The new file with an `id:` on every task line — what the projection must become.
    pub file: File,
    /// Ids minted for lines that matched nothing.
    pub minted: usize,
    /// Ids recovered by content for lines whose tag was stripped.
    pub reused: usize,
}

/// Derives ops that turn `old` (our last projection) into `new` (the bytes on disk).
pub fn reconcile(
    old: &File,
    new: &File,
    path: &FilePath,
    mint: &mut dyn FnMut() -> TaskId,
) -> Reconciled {
    let (file, minted, reused) = assign_ids(old, new, mint);
    let old_ids: Vec<Option<TaskId>> = old.lines.iter().map(id_of).collect();
    let new_ids: Vec<Option<TaskId>> = file.lines.iter().map(id_of).collect();
    let diffs = diff_lines(old, &file);
    let mut ops = Vec::new();
    delete_pass(&mut ops, &diffs, old, &old_ids);
    change_pass(&mut ops, &diffs, old, &file);
    task_pass(&mut ops, &diffs, &file, &new_ids, path);
    blank_pass(&mut ops, &diffs, &new_ids);
    debug_assert!(
        ops.len() <= 6 * file.lines.len() + old.lines.len(),
        "op count is bounded by the line counts"
    );
    debug_assert!(
        file.lines.iter().all(|l| !is_task(l) || id_of(l).is_some()),
        "every task line has an id"
    );
    Reconciled {
        ops,
        file,
        minted,
        reused,
    }
}

/// Field-level ops for a same-id line whose bytes changed: priority first (so a completion that
/// dropped `(A)` does not move it to `pri:`), then dates, completion, then the description.
pub fn change_ops(old: &OwnedLine, new: &OwnedLine) -> Vec<OpKind> {
    let (Some(a), Some(b)) = (task_of(old), task_of(new)) else {
        return Vec::new();
    };
    let Some(task) = a.id().map(TaskId::new) else {
        return Vec::new();
    };
    debug_assert_eq!(a.id(), b.id(), "change_ops is for same-id lines");
    let mut ops = Vec::new();
    let mut push = |field: Field, value: FieldValue| {
        if let Ok(op) = set_field(task, field, value) {
            ops.push(op);
        }
    };
    if a.priority != b.priority {
        push(Field::Priority, FieldValue::priority(b.priority));
    }
    if a.creation_date != b.creation_date {
        push(Field::CreationDate, FieldValue::date(b.creation_date));
    }
    if a.completed != b.completed {
        push(Field::Completed, FieldValue::Bool(b.completed));
    }
    if a.completion_date != b.completion_date {
        push(Field::CompletionDate, FieldValue::date(b.completion_date));
    }
    if a.description != b.description {
        let edits: Vec<TextEdit> = diff_text(a.description, b.description)
            .into_iter()
            .map(TextEdit::from)
            .collect();
        ops.push(OpKind::EditText { task, edits });
    }
    ops
}

/// Pass 1: deletions, bottom-up so a blank's anchor task is still present when the blank goes.
fn delete_pass(ops: &mut Vec<OpKind>, diffs: &[LineDiff], old: &File, old_ids: &[Option<TaskId>]) {
    for d in diffs.iter().rev() {
        if let LineDiff::Delete { from } = d {
            ops.push(delete_op(old, old_ids, *from));
        }
    }
}

/// Pass 2: same-id lines whose bytes changed.
fn change_pass(ops: &mut Vec<OpKind>, diffs: &[LineDiff], old: &File, file: &File) {
    for d in diffs {
        if let LineDiff::Change { from, to } = d {
            ops.extend(change_ops(&old.lines[*from], &file.lines[*to]));
        }
    }
}

/// Pass 3: new and moved task lines, top-down, each placed right after its predecessor task.
fn task_pass(
    ops: &mut Vec<OpKind>,
    diffs: &[LineDiff],
    file: &File,
    new_ids: &[Option<TaskId>],
    path: &FilePath,
) {
    for d in diffs {
        match d {
            LineDiff::Insert { to } if new_ids[*to].is_some() => {
                ops.push(insert_op(file, new_ids, *to))
            }
            LineDiff::Move { to, .. } => {
                if let Some(task) = new_ids[*to] {
                    ops.push(OpKind::Move {
                        task,
                        after: prev_task(new_ids, *to),
                        to_file: path.clone(),
                    });
                }
            }
            LineDiff::Insert { .. }
            | LineDiff::Keep { .. }
            | LineDiff::Delete { .. }
            | LineDiff::Change { .. } => {}
        }
    }
}

/// Pass 4: new blank lines, after every task is in place, each anchored to the task above it.
fn blank_pass(ops: &mut Vec<OpKind>, diffs: &[LineDiff], new_ids: &[Option<TaskId>]) {
    for d in diffs {
        if let LineDiff::Insert { to } = d
            && new_ids[*to].is_none()
        {
            ops.push(OpKind::BlankInsert {
                after: prev_task(new_ids, *to),
            });
        }
    }
}

fn delete_op(old: &File, old_ids: &[Option<TaskId>], from: usize) -> OpKind {
    match old_ids[from] {
        Some(task) => {
            set_field(task, Field::Deleted, FieldValue::Bool(true)).unwrap_or(OpKind::BlankRemove {
                after: prev_task(old_ids, from),
            })
        }
        None => {
            debug_assert!(
                !is_task(&old.lines[from]),
                "old projection task lines always carry ids"
            );
            OpKind::BlankRemove {
                after: prev_task(old_ids, from),
            }
        }
    }
}

fn insert_op(file: &File, new_ids: &[Option<TaskId>], to: usize) -> OpKind {
    let after = prev_task(new_ids, to);
    match new_ids[to] {
        Some(task) => OpKind::Insert {
            task,
            after,
            line: text_of(&file.lines[to]),
        },
        None => OpKind::BlankInsert { after },
    }
}

/// The nearest task id above index `i`, or `None` at the top.
fn prev_task(ids: &[Option<TaskId>], i: usize) -> Option<TaskId> {
    debug_assert!(i <= ids.len());
    ids[..i].iter().rev().find_map(|id| *id)
}

/// Gives every id-less task line an id: recovered from an old line with the same text minus its
/// tag when one is unclaimed, else minted. Returns the completed file and the two counts.
fn assign_ids(old: &File, new: &File, mint: &mut dyn FnMut() -> TaskId) -> (File, usize, usize) {
    let mut by_text: BTreeMap<String, VecDeque<TaskId>> = BTreeMap::new();
    for line in &old.lines {
        if let (Some(id), Some(text)) = (id_of(line), text_without_id(line)) {
            by_text.entry(text).or_default().push_back(id);
        }
    }
    let mut file = new.clone();
    let (mut minted, mut reused) = (0usize, 0usize);
    for line in file
        .lines
        .iter_mut()
        .filter(|l| is_task(l) && id_of(l).is_none())
    {
        let recovered = line
            .raw()
            .and_then(|t| by_text.get_mut(t))
            .and_then(VecDeque::pop_front);
        let id = match recovered {
            Some(id) => {
                reused += 1;
                id
            }
            None => {
                minted += 1;
                mint()
            }
        };
        let tag = id.ulid().to_string();
        if let Ok(edit) = Edit::new().set_tag("id", &tag) {
            *line = txtodo_core::apply(line, &edit);
        }
    }
    debug_assert!(minted + reused <= new.lines.len());
    (file, minted, reused)
}

fn task_of(line: &OwnedLine) -> Option<Task<'_>> {
    match line.parse()?.kind {
        LineKind::Task(t) => Some(t),
        LineKind::Blank => None,
    }
}

fn is_task(line: &OwnedLine) -> bool {
    task_of(line).is_some()
}

fn text_of(line: &OwnedLine) -> String {
    line.raw().unwrap_or_default().to_owned()
}

/// The line text with its `id:` word removed, for content matching.
fn text_without_id(line: &OwnedLine) -> Option<String> {
    let edit = Edit::new().remove_tag("id").ok()?;
    txtodo_core::apply(line, &edit).raw().map(str::to_owned)
}
