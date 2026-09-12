//! History as a view (design §4.8): replay a document to a log position, render it at a wall
//! time, and invert ops for undo. Replay starts at the newest snapshot at or before the target
//! and applies the ops after it, so an adopted (non-op) state is honoured too.

use crate::handle::ActorError;
use crate::state::DocState;
use crate::textedit::apply_text_edits;
use txtodo_core::{File, LineKind, parse_file};
use txtodo_model::{Field, FieldValue, FilePath, OpKind, TaskId, TextEdit, set_field};
use txtodo_store::{MAX_OPS_PER_READ, Seq, Store, Stored};

/// Most read pages one replay walks; 1000 × MAX_OPS_PER_READ ops is far past any todo list.
pub const MAX_REPLAY_PAGES: usize = 1_000;

/// The document state after every op with `seq <= upto` (all ops when `upto` is `None`).
pub fn replay(store: &Store, path: &FilePath, upto: Option<Seq>) -> Result<DocState, ActorError> {
    let target = match upto {
        Some(s) => s,
        None => store.last_seq()?.unwrap_or(Seq(0)),
    };
    let snapshot = store.snapshot_at_or_before(path, target)?;
    let (mut since, base) = match snapshot {
        Some(s) => (s.seq, parse_file(&s.state)),
        None => (Seq(0), File::default()),
    };
    let mut state = DocState::from_tagged_file(path.clone(), &base)?;
    for _page in 0..MAX_REPLAY_PAGES {
        let ops = store.for_file(path, since)?;
        let Some(last) = ops.last() else { break };
        for stored in ops.iter().take_while(|s| s.seq <= target) {
            state.apply(&stored.op)?;
        }
        since = last.seq;
        if last.seq >= target || ops.len() < MAX_OPS_PER_READ {
            break;
        }
    }
    debug_assert!(state.to_bytes().len() <= txtodo_store::MAX_PROJECTION_BYTES);
    Ok(state)
}

/// The newest seq whose op has `hlc.wall_ms <= at_wall_ms`, or `None` when nothing is that old.
pub fn seq_at_wall(
    store: &Store,
    path: &FilePath,
    at_wall_ms: u64,
) -> Result<Option<Seq>, ActorError> {
    let mut since = Seq(0);
    let mut best: Option<Seq> = None;
    for _page in 0..MAX_REPLAY_PAGES {
        let ops = store.for_file(path, since)?;
        let Some(last) = ops.last() else { break };
        // One device: hlc wall time is monotone in seq, so the first newer op ends the scan.
        for s in &ops {
            if s.op.hlc.wall_ms <= at_wall_ms {
                best = Some(s.seq);
            } else {
                return Ok(best);
            }
        }
        since = last.seq;
        if ops.len() < MAX_OPS_PER_READ {
            break;
        }
    }
    Ok(best)
}

/// The document bytes as they were at `at_wall_ms` (inclusive).
pub fn checkout(store: &Store, path: &FilePath, at_wall_ms: u64) -> Result<Vec<u8>, ActorError> {
    let Some(seq) = seq_at_wall(store, path, at_wall_ms)? else {
        return Ok(Vec::new());
    };
    Ok(replay(store, path, Some(seq))?.to_bytes())
}

/// The op that undoes `stored`, given the state just before it. `None` for ops with no inverse
/// on one device (NotesEdit) or when the state does not hold the task (already gone).
pub fn inverse(before: &DocState, stored: &Stored) -> Option<OpKind> {
    match &stored.op.kind {
        OpKind::Insert { task, .. } => {
            set_field(*task, Field::Deleted, FieldValue::Bool(true)).ok()
        }
        OpKind::SetField {
            task,
            field: Field::Deleted,
            ..
        } => {
            let i = before.index_of(*task)?;
            let after = before.task_before(i);
            let line = before.line_of(*task)?.raw()?.to_owned();
            Some(OpKind::Insert {
                task: *task,
                after,
                line,
            })
        }
        OpKind::SetField { task, field, .. } => inverse_set_field(before, *task, *field),
        OpKind::EditText { task, edits } => inverse_edit_text(before, *task, edits),
        OpKind::Move { task, to_file, .. } => {
            let after = before.task_before(before.index_of(*task)?);
            Some(OpKind::Move {
                task: *task,
                after,
                to_file: to_file.clone(),
            })
        }
        OpKind::BlankInsert { after } => Some(OpKind::BlankRemove { after: *after }),
        OpKind::BlankRemove { after } => Some(OpKind::BlankInsert { after: *after }),
        OpKind::NotesEdit { .. } => None,
    }
}

/// The previous value of a prefix field, read from the state before the op.
fn inverse_set_field(before: &DocState, task: TaskId, field: Field) -> Option<OpKind> {
    let line = before.line_of(task)?;
    let LineKind::Task(t) = line.parse()?.kind else {
        return None;
    };
    let value = match field {
        Field::Completed => FieldValue::Bool(t.completed),
        Field::CompletionDate => FieldValue::date(t.completion_date),
        Field::CreationDate => FieldValue::date(t.creation_date),
        Field::Priority => FieldValue::priority(t.priority),
        Field::Quirks => FieldValue::quirks(line.quirks()),
        Field::Deleted => return None,
    };
    set_field(task, field, value).ok()
}

/// `diff_text(after, before)` — the edit stream that takes the description back.
fn inverse_edit_text(before: &DocState, task: TaskId, edits: &[TextEdit]) -> Option<OpKind> {
    let line = before.line_of(task)?;
    let LineKind::Task(t) = line.parse()?.kind else {
        return None;
    };
    let old = t.description.to_owned();
    let new = apply_text_edits(&old, edits).ok()?;
    let back: Vec<TextEdit> = txtodo_core::diff_text(&new, &old)
        .into_iter()
        .map(TextEdit::from)
        .collect();
    debug_assert!(apply_text_edits(&new, &back).ok().as_deref() == Some(old.as_str()));
    Some(OpKind::EditText { task, edits: back })
}

/// Inverse ops for the newest `steps` ops of `path`, newest first, each computed against the
/// state just before its op. Ops without an inverse are skipped.
pub fn undo_ops(store: &Store, path: &FilePath, steps: u16) -> Result<Vec<OpKind>, ActorError> {
    let steps = usize::from(steps.max(1));
    let newest = store.newest(path, steps)?;
    let mut out = Vec::with_capacity(newest.len());
    for stored in &newest {
        let before = replay(store, path, Some(Seq(stored.seq.0 - 1)))?;
        if let Some(inv) = inverse(&before, stored) {
            out.push(inv);
        }
    }
    debug_assert!(out.len() <= steps);
    Ok(out)
}
