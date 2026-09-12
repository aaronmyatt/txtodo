//! Converging the Loro mirror onto an adopted state with corrective ops, keeping its lineage.
//! The reconciler cannot do this: it anchors an insert to the previous *task*, so a line that
//! sits after a blank lands before it. Here the anchor is the previous *entry* — a task id or the
//! mirror's own blank sentinel — because the mirror's list ids are known. One forward walk over
//! the state fixes order, inserts and blanks; deletes and rewrites come first. Every op is
//! stamped `hlc` and never enters the log: it is the mirror catching up, not a change.

use crate::mirror::{Mirror, MirrorError};
use crate::state::{DocState, Entry};
use txtodo_crdt::{is_blank, rebuild_line};
use txtodo_model::{Field, FieldValue, Hlc, Op, OpId, OpKind, Principal, TaskId, Ulid, set_field};

impl Mirror {
    /// Brings the mirror to the state; returns the number of corrective ops it took.
    pub fn converge_to(&mut self, state: &DocState, hlc: Hlc) -> Result<usize, MirrorError> {
        let want: Vec<Entry> = (0..state.len()).filter_map(|i| state.entry_at(i)).collect();
        debug_assert_eq!(want.len(), state.len());
        let mut visible = self.visible_ids();
        let mut ops = delete_missing(&mut visible, state);
        // Order, inserts and blanks in one walk; `prev` is the entry the next one follows.
        let mut prev: Option<TaskId> = None;
        for entry in &want {
            let (op, id) = place(entry, prev, &mut visible, state);
            ops.extend(op);
            prev = Some(id);
        }
        ops.extend(trim_blanks(&mut visible, want.len()));
        self.apply_corrections(state, &ops, hlc)?;
        debug_assert!(self.agrees_with(state), "converged");
        Ok(ops.len())
    }

    /// Applies the corrective ops one at a time, resolving the placeholder anchor to the sentinel
    /// the previous `BlankInsert` minted, then settles descriptions (a canonical line may differ
    /// from the raw one in ways the text must follow).
    fn apply_corrections(
        &mut self,
        state: &DocState,
        kinds: &[OpKind],
        hlc: Hlc,
    ) -> Result<(), MirrorError> {
        let mut last_blank: Option<TaskId> = None;
        // Bounded by the corrective op list, itself ≤ 3 × the document length.
        for kind in kinds {
            let kind = resolve_anchor(kind.clone(), last_blank);
            let op = Op {
                id: OpId::new(Ulid::from_u128(0)),
                hlc,
                principal: Principal::External { device: hlc.device },
                file: state.path().clone(),
                kind,
            };
            self.replay(std::slice::from_ref(&op))?;
            if let OpKind::Insert { task, .. } | OpKind::EditText { task, .. } = op.kind {
                self.settle_description(task, state)?;
            }
            if matches!(op.kind, OpKind::BlankInsert { .. }) {
                last_blank = self.doc().last_blank_id();
            }
        }
        debug_assert!(kinds.is_empty() || !self.visible_ids().is_empty() || state.is_empty());
        Ok(())
    }

    /// The mirror's visible ids: blanks and live tasks, tombstones skipped.
    pub(crate) fn visible_ids(&self) -> Vec<TaskId> {
        let ids: Vec<TaskId> = self
            .doc()
            .list_ids(self.path())
            .into_iter()
            .filter(|id| is_blank(*id) || !self.doc().is_deleted(*id))
            .collect();
        debug_assert!(
            ids.iter()
                .all(|id| is_blank(*id) || rebuild_line(self.doc(), *id).is_ok())
        );
        ids
    }
}

/// Deletes every live task the state no longer has; they drop out of `visible` as tombstones.
fn delete_missing(visible: &mut Vec<TaskId>, state: &DocState) -> Vec<OpKind> {
    let gone: Vec<TaskId> = visible
        .iter()
        .copied()
        .filter(|id| !is_blank(*id) && state.index_of(*id).is_none())
        .collect();
    visible.retain(|v| !gone.contains(v));
    debug_assert!(
        visible
            .iter()
            .all(|v| is_blank(*v) || state.index_of(*v).is_some())
    );
    gone.into_iter()
        .map(|id| {
            set_field(id, Field::Deleted, FieldValue::Bool(true))
                .unwrap_or(OpKind::BlankRemove { after: None })
        })
        .collect()
}

/// Puts `entry` right after `prev` in `visible`, emitting the op that does it in the mirror (none
/// when it is already there). Returns the id the next entry will follow — for a fresh blank the
/// placeholder, resolved to the minted sentinel at apply time.
fn place(
    entry: &Entry,
    prev: Option<TaskId>,
    visible: &mut Vec<TaskId>,
    state: &DocState,
) -> (Option<OpKind>, TaskId) {
    let at = prev.map_or(0, |p| position(visible, p) + 1);
    debug_assert!(at <= visible.len());
    match entry {
        Entry::Task { id, line } => {
            if visible.get(at) == Some(id) {
                return (None, *id);
            }
            let op = if visible.contains(id) {
                visible.retain(|v| v != id);
                OpKind::Move {
                    task: *id,
                    after: prev,
                    to_file: state.path().clone(),
                }
            } else {
                OpKind::Insert {
                    task: *id,
                    after: prev,
                    line: line.raw().unwrap_or_default().to_owned(),
                }
            };
            visible.insert(at.min(visible.len()), *id);
            (Some(op), *id)
        }
        Entry::Blank(_) => match visible.get(at).copied().filter(|s| is_blank(*s)) {
            Some(s) => (None, s),
            None => {
                visible.insert(at, PLACEHOLDER);
                (Some(OpKind::BlankInsert { after: prev }), PLACEHOLDER)
            }
        },
    }
}

/// Removes the blanks left past the state's end (only blanks can be left: tasks were deleted).
fn trim_blanks(visible: &mut Vec<TaskId>, keep: usize) -> Vec<OpKind> {
    let mut ops = Vec::new();
    while visible.len() > keep {
        let extra = visible[keep];
        debug_assert!(is_blank(extra), "only blanks can be left over");
        let after = if keep == 0 {
            None
        } else {
            Some(visible[keep - 1])
        };
        ops.push(OpKind::BlankRemove { after });
        visible.remove(keep);
    }
    debug_assert!(visible.len() <= keep);
    ops
}

/// Stands in for a sentinel the mirror has not minted yet; the top byte marks it as a blank.
const PLACEHOLDER: TaskId = TaskId::new(Ulid::from_u128(0xFFFF_FFFF_FFFF_FFFF_FFFF_FFFF_FFFF_FFFF));

/// Replaces a placeholder anchor with the sentinel the last `BlankInsert` produced.
fn resolve_anchor(kind: OpKind, last_blank: Option<TaskId>) -> OpKind {
    let fix = |after: Option<TaskId>| {
        if after == Some(PLACEHOLDER) {
            last_blank
        } else {
            after
        }
    };
    match kind {
        OpKind::Insert { task, after, line } => OpKind::Insert {
            task,
            after: fix(after),
            line,
        },
        OpKind::Move {
            task,
            after,
            to_file,
        } => OpKind::Move {
            task,
            after: fix(after),
            to_file,
        },
        OpKind::BlankInsert { after } => OpKind::BlankInsert { after: fix(after) },
        OpKind::BlankRemove { after } => OpKind::BlankRemove { after: fix(after) },
        other @ (OpKind::SetField { .. } | OpKind::EditText { .. } | OpKind::NotesEdit { .. }) => {
            other
        }
    }
}

fn position(visible: &[TaskId], id: TaskId) -> usize {
    let at = visible.iter().position(|v| *v == id);
    debug_assert!(at.is_some(), "an anchor is always in the visible list");
    at.unwrap_or(visible.len().saturating_sub(1))
}
