//! File → ops (design §4.3 steps 3–6, one device): the bytes just read against the bytes we last
//! wrote, keyed by `id:`, become ops the actor applies to its state. Pure: no I/O, no clock — the
//! caller supplies a minting closure for fresh ids. The actor checks the postcondition
//! `apply(ops) == file` and adopts the file if a quirky line defeats the op model.
//!
//! Four passes: deletes (bottom-up), same-id changes, task inserts and moves (top-down, each right
//! after its predecessor task), then new blank lines anchored to the task above them.
//! Hot path: ids come from `fast_id_of`; the content index for stripped ids is built only when a
//! line actually lacks one (budget: one edit in 10k lines ≤ 20 ms, plan §5).

use crate::fastid::fast_id_of;
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
    /// Every line's task id in `file`'s order, `None` for a blank — what `DocState::from_file`
    /// takes instead of re-deriving ids from text (sidecar mode has no `id:` tag to derive from).
    pub ids: Vec<Option<TaskId>>,
}

/// Derives ops that turn `old` (our last projection) into `new` (the bytes on disk).
pub fn reconcile(
    old: &File,
    new: &File,
    path: &FilePath,
    mint: &mut dyn FnMut() -> TaskId,
) -> Reconciled {
    let (file, minted, reused) = assign_ids(old, new, mint);
    let old_ids: Vec<Option<TaskId>> = old.lines.iter().map(fast_id_of).collect();
    let new_ids: Vec<Option<TaskId>> = file.lines.iter().map(fast_id_of).collect();
    let diffs = diff_lines(old, &file);
    let mut ops = Vec::new();
    delete_pass(&mut ops, &diffs, old, &old_ids);
    change_pass(&mut ops, &diffs, old, &old_ids, &file);
    task_pass(&mut ops, &diffs, &file, &new_ids, path);
    blank_pass(&mut ops, &diffs, &new_ids);
    debug_assert!(
        ops.len() <= 6 * file.lines.len() + old.lines.len(),
        "op count is bounded by the line counts"
    );
    debug_assert!(
        new_ids
            .iter()
            .zip(&file.lines)
            .all(|(id, l)| id.is_some() || !is_task(l)),
        "every task line has an id"
    );
    Reconciled {
        ops,
        file,
        minted,
        reused,
        ids: new_ids,
    }
}

/// Field-level ops for a changed line paired to `task` by the caller (an id read off the line in
/// tagged mode, a fingerprint match in sidecar mode — this function doesn't care which): priority
/// first (so a completion that dropped `(A)` does not move it to `pri:`), then dates, completion,
/// then the description.
pub fn change_ops(old: &OwnedLine, new: &OwnedLine, task: TaskId) -> Vec<OpKind> {
    let (Some(a), Some(b)) = (task_of(old), task_of(new)) else {
        return Vec::new();
    };
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
pub(crate) fn delete_pass(
    ops: &mut Vec<OpKind>,
    diffs: &[LineDiff],
    old: &File,
    old_ids: &[Option<TaskId>],
) {
    for d in diffs.iter().rev() {
        if let LineDiff::Delete { from } = d {
            ops.push(delete_op(old, old_ids, *from));
        }
    }
}

/// Pass 2: changed lines paired by the caller's diff, each already resolved to the task it was
/// (`old_ids[from]`, `None` skips it — a changed line with no id has nothing to stamp ops against).
pub(crate) fn change_pass(
    ops: &mut Vec<OpKind>,
    diffs: &[LineDiff],
    old: &File,
    old_ids: &[Option<TaskId>],
    file: &File,
) {
    for d in diffs {
        if let LineDiff::Change { from, to } = d
            && let Some(task) = old_ids[*from]
        {
            ops.extend(change_ops(&old.lines[*from], &file.lines[*to], task));
        }
    }
}

/// Pass 3: new and moved task lines, top-down, each placed right after its predecessor task.
pub(crate) fn task_pass(
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
pub(crate) fn blank_pass(ops: &mut Vec<OpKind>, diffs: &[LineDiff], new_ids: &[Option<TaskId>]) {
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
pub(crate) fn prev_task(ids: &[Option<TaskId>], i: usize) -> Option<TaskId> {
    debug_assert!(i <= ids.len());
    ids[..i].iter().rev().find_map(|id| *id)
}

/// Gives every id-less task line an id: recovered from an old line with the same text minus its
/// tag when one is unclaimed, else minted. Returns the completed file and the two counts. The
/// content index over `old` is built only when some new line needs it.
fn assign_ids(old: &File, new: &File, mint: &mut dyn FnMut() -> TaskId) -> (File, usize, usize) {
    let needs: Vec<usize> = (0..new.lines.len())
        .filter(|&i| fast_id_of(&new.lines[i]).is_none() && is_task(&new.lines[i]))
        .collect();
    if needs.is_empty() {
        return (new.clone(), 0, 0);
    }
    let mut by_text: BTreeMap<String, VecDeque<TaskId>> = BTreeMap::new();
    for line in &old.lines {
        if let (Some(id), Some(text)) = (fast_id_of(line), text_without_id(line)) {
            by_text.entry(text).or_default().push_back(id);
        }
    }
    let mut file = new.clone();
    let (mut minted, mut reused) = (0usize, 0usize);
    for i in needs {
        let line = &mut file.lines[i];
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

pub(crate) fn task_of(line: &OwnedLine) -> Option<Task<'_>> {
    match line.parse()?.kind {
        LineKind::Task(t) => Some(t),
        LineKind::Blank => None,
    }
}

pub(crate) fn is_task(line: &OwnedLine) -> bool {
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
