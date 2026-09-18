//! Converging the Loro mirror onto an adopted state with corrective ops, keeping its lineage.
//! The reconciler cannot do this: it anchors an insert to the previous *task*, so a line that
//! sits after a blank lands before it. Here the anchor is the previous *entry* — a task id or the
//! mirror's own blank sentinel — because the mirror's list ids are known. One forward walk over
//! the state fixes order, inserts and blanks; deletes and rewrites come first. Every op is
//! stamped `hlc` and never enters the log: it is the mirror catching up, not a change.

use std::collections::HashMap;

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
        let mut ops: Vec<(OpKind, Option<TaskId>)> = delete_missing(&mut visible, state)
            .into_iter()
            .map(|k| (k, None))
            .collect();
        // Order, inserts and blanks in one walk; `prev` is the entry the next one follows.
        let mut prev: Option<TaskId> = None;
        let mut placeholders: u128 = u128::MAX;
        for entry in &want {
            let (op, id) = place(entry, prev, &mut visible, state, &mut placeholders);
            if let Some(op) = op {
                // Only a fresh `BlankInsert` defines a placeholder that later anchors need
                // resolved; an "already there" match or a Task op never does.
                let defines = matches!(op, OpKind::BlankInsert { .. }).then_some(id);
                ops.push((op, defines));
            }
            prev = Some(id);
        }
        ops.extend(
            trim_blanks(&mut visible, want.len())
                .into_iter()
                .map(|k| (k, None)),
        );
        self.apply_corrections(state, &ops, hlc)?;
        debug_assert!(self.agrees_with(state), "converged");
        Ok(ops.len())
    }

    /// Applies the corrective ops one at a time, resolving each placeholder anchor to the real
    /// sentinel *its own* `BlankInsert` minted (task `daemon-mirror-assertion-panic`: tracking
    /// only "the last blank minted so far" broke once two or more placeholders needed resolving
    /// in the same walk), then settles descriptions (a canonical line may differ from the raw one
    /// in ways the text must follow).
    fn apply_corrections(
        &mut self,
        state: &DocState,
        ops: &[(OpKind, Option<TaskId>)],
        hlc: Hlc,
    ) -> Result<(), MirrorError> {
        let mut resolved: HashMap<TaskId, TaskId> = HashMap::new();
        // Bounded by the corrective op list, itself ≤ 3 × the document length.
        for (kind, defines) in ops {
            let kind = resolve_anchor(kind.clone(), &resolved);
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
            if let Some(placeholder) = defines
                && let Some(real) = self.doc().last_blank_id()
            {
                resolved.insert(*placeholder, real);
            }
        }
        debug_assert!(ops.is_empty() || !self.visible_ids().is_empty() || state.is_empty());
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
/// when it is already there). Returns the id the next entry will follow — for a fresh blank a
/// freshly minted placeholder (from `placeholders`), resolved to the real sentinel at apply time.
fn place(
    entry: &Entry,
    prev: Option<TaskId>,
    visible: &mut Vec<TaskId>,
    state: &DocState,
    placeholders: &mut u128,
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
                let placeholder = mint_placeholder(placeholders);
                visible.insert(at, placeholder);
                (Some(OpKind::BlankInsert { after: prev }), placeholder)
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

/// Mints a fresh not-yet-real blank stand-in each call — walk-local, valid only until
/// `apply_corrections` resolves it via its own `resolved` map. Task
/// `daemon-mirror-assertion-panic`: a single shared sentinel (the old `PLACEHOLDER` constant)
/// could not be told apart from itself once two or more coexisted in one walk's `visible`
/// simulation, so `position()` (`.iter().position`, first-match) silently resolved to a *stale*
/// occurrence — the third+ new blank in a row in the same walk read as "already there" against
/// an earlier placeholder's slot, and its `BlankInsert` was never emitted at all. Counting down
/// from `u128::MAX` only ever changes the low 120 bits for any realistic document (`txtodo-daemon`
/// bounds documents at `MAX_LINES_PER_FILE`, far short of 2^120), so it never touches the top
/// byte `is_blank` checks, and never collides with a real blank's small, upward-growing
/// `next_blank` counter.
fn mint_placeholder(next: &mut u128) -> TaskId {
    let id = TaskId::new(Ulid::from_u128(*next));
    debug_assert!(is_blank(id), "a placeholder must still look like a blank");
    *next -= 1;
    id
}

/// Replaces a placeholder anchor with the real sentinel `resolved` recorded for it, if any.
fn resolve_anchor(kind: OpKind, resolved: &HashMap<TaskId, TaskId>) -> OpKind {
    let fix = |after: Option<TaskId>| after.map(|a| resolved.get(&a).copied().unwrap_or(a));
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
