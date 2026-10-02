//! History as a view (design §4.8): replay a document to a log position, render it at a wall
//! time, and invert ops for undo. Replay starts at the newest snapshot at or before the target
//! and applies the ops after it, so an adopted (non-op) state is honoured too.

use crate::fastid::fast_id_of;
use crate::handle::ActorError;
use crate::reconcile::{change_ops, task_of};
use crate::state::DocState;
use crate::textedit::apply_text_edits;
use txtodo_core::{File, LineKind, parse_file};
use txtodo_model::{
    Field, FieldValue, FilePath, IdentityMode, OpKind, TaskId, TextEdit, set_field,
};
use txtodo_store::{MAX_OPS_PER_READ, Seq, Store, Stored};

/// Most read pages one replay walks; 1000 × MAX_OPS_PER_READ ops is far past any todo list.
pub const MAX_REPLAY_PAGES: usize = 1_000;

/// The document state after every op with `seq <= upto` (all ops when `upto` is `None`). Ops
/// carry their own task ids (`OpKind::Insert{task,..}` etc.), so replay itself needs no id
/// resolution; only the *starting* `base` (empty, or a snapshot) does — see `resolve_base_ids`.
pub fn replay(
    store: &Store,
    path: &FilePath,
    upto: Option<Seq>,
    mode: IdentityMode,
) -> Result<DocState, ActorError> {
    let target = match upto {
        Some(s) => s,
        None => store.last_seq()?.unwrap_or(Seq(0)),
    };
    let snapshot = store.snapshot_at_or_before(path, target)?;
    let (mut since, base) = match snapshot {
        Some(s) => (s.seq, parse_file(&s.state)),
        None => (Seq(0), File::default()),
    };
    let ids = resolve_base_ids(store, path, &base, mode)?;
    let mut state = DocState::from_file(path.clone(), &base, &ids, mode)?;
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

/// Every line's id for `base`, the state replay starts from: tagged mode reads them off the text
/// (`fast_id_of`), same as always. Sidecar mode has no text to read, so this falls back to the
/// live fingerprints — exact whenever `base` is the current live state (the common case: no
/// snapshot yet, or the newest one), best-effort for an older snapshot, since fingerprints are
/// not themselves versioned by history. `None` per line, not an error, when nothing lines up
/// (`DocState::from_file` reports it as `MissingId`).
fn resolve_base_ids(
    store: &Store,
    path: &FilePath,
    base: &File,
    mode: IdentityMode,
) -> Result<Vec<Option<TaskId>>, ActorError> {
    if mode == IdentityMode::Tagged {
        return Ok(base.lines.iter().map(fast_id_of).collect());
    }
    let mut rows = store.live_fingerprints(path)?.into_iter();
    Ok(base
        .lines
        .iter()
        .map(|line| {
            if task_of(line).is_none() {
                None
            } else {
                rows.next().map(|row| row.task)
            }
        })
        .collect())
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
pub fn checkout(
    store: &Store,
    path: &FilePath,
    at_wall_ms: u64,
    mode: IdentityMode,
) -> Result<Vec<u8>, ActorError> {
    let Some(seq) = seq_at_wall(store, path, at_wall_ms)? else {
        return Ok(Vec::new());
    };
    Ok(replay(store, path, Some(seq), mode)?.to_bytes())
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
        OpKind::RemoveTag { task, key } => inverse_remove_tag(before, *task, key),
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

/// The text edit that puts back the tag `RemoveTag` dropped; `None` when it dropped nothing.
fn inverse_remove_tag(before: &DocState, task: TaskId, key: &str) -> Option<OpKind> {
    let line = before.line_of(task)?;
    let LineKind::Task(t) = line.parse()?.kind else {
        return None;
    };
    let after = txtodo_model::remove_tag(t.description, key);
    (after != t.description).then(|| OpKind::EditText {
        task,
        edits: txtodo_core::diff_text(&after, t.description)
            .into_iter()
            .map(TextEdit::from)
            .collect(),
    })
}

/// Inverse ops for the newest `steps` ops of `path`, newest first, each computed against the
/// state just before its op. Ops without an inverse are skipped; a completion's inverse is
/// several ops (`inverse_completion`), so the result can be longer than `steps`.
pub fn undo_ops(
    store: &Store,
    path: &FilePath,
    steps: u16,
    mode: IdentityMode,
) -> Result<Vec<OpKind>, ActorError> {
    let steps = usize::from(steps.max(1));
    let newest = store.newest(path, steps)?;
    let mut out = Vec::with_capacity(newest.len());
    for stored in &newest {
        let before = replay(store, path, Some(Seq(stored.seq.0 - 1)), mode)?;
        if let Some(reopen) = inverse_completion(&before, stored) {
            out.extend(reopen);
        } else if let Some(inv) = inverse(&before, stored) {
            out.push(inv);
        }
    }
    Ok(out)
}

/// Undoing a completion is a reopen. A do sends `Completed` alone and its `(X)` moves into
/// `pri:X` as a side effect (`reconcile::change_ops`), so a lone `Completed=false` would leave
/// the tag behind. Diff the line after the op back to the line before it, as `reopen_ops` does:
/// `Completed=false`, the priority, and a text edit dropping `pri:`. `None` for any other op.
fn inverse_completion(before: &DocState, stored: &Stored) -> Option<Vec<OpKind>> {
    let OpKind::SetField {
        task,
        field: Field::Completed,
        value: FieldValue::Bool(true),
    } = &stored.op.kind
    else {
        return None;
    };
    let old = before.line_of(*task)?;
    let mut after = before.clone();
    after.apply(&stored.op).ok()?;
    let new = after.line_of(*task)?;
    Some(change_ops(&new, &old, *task))
}

/// History defaults/caps (design §4.8): unset page size, and the hard cap per call.
pub(crate) const HISTORY_DEFAULT_LIMIT: usize = 50;
pub(crate) const HISTORY_MAX_LIMIT: usize = 1_000;

impl crate::server::TxtodoService {
    /// `History` RPC (design §4.8): ops newest first, filtered by path and/or task. Split out of
    /// `server.rs` for its line budget — the same `*_impl` pattern as `tokens.rs`/`pairing_grpc.rs`.
    pub(crate) async fn history_impl(
        &self,
        r: tonic::Request<txtodo_proto::v1::HistoryRequest>,
    ) -> Result<tonic::Response<txtodo_proto::v1::HistoryResponse>, tonic::Status> {
        let req = r.get_ref();
        let limit = if req.limit == 0 {
            HISTORY_DEFAULT_LIMIT
        } else {
            (req.limit as usize).min(HISTORY_MAX_LIMIT)
        };
        let task = crate::convert::parse_ulid_opt(&req.task_id)?.map(TaskId::new);
        let paths: Vec<FilePath> = if req.path.is_empty() {
            self.workspace().paths().cloned().collect()
        } else {
            vec![crate::convert::parse_path(&req.path)?]
        };
        let ws = self.workspace();
        let store = ws
            .store()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut rows = Vec::new();
        for p in &paths {
            let newest = store
                .newest(p, txtodo_store::MAX_OPS_PER_READ)
                .map_err(|e| tonic::Status::internal(e.to_string()))?;
            rows.extend(
                newest
                    .into_iter()
                    .filter(|s| req.before_seq == 0 || s.seq.0 < req.before_seq),
            );
        }
        rows.retain(|s| task.is_none_or(|t| crate::convert::task_of(&s.op.kind) == Some(t)));
        rows.sort_by_key(|s| std::cmp::Reverse(s.seq));
        rows.truncate(limit);
        debug_assert!(rows.len() <= limit);
        // One query over the rows' seq span, for the client that made each change (task
        // op-source); the same helper the activity stream uses.
        let sources = crate::activity::sources_for_rows(&store, &rows)?;
        let ops = rows
            .iter()
            .map(|s| {
                let mut summary = crate::convert::to_summary(s);
                summary.source = sources.get(&s.seq).cloned().unwrap_or_default();
                summary
            })
            .collect();
        Ok(tonic::Response::new(txtodo_proto::v1::HistoryResponse {
            ops,
        }))
    }
}
