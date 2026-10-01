//! A task inserted again (task partition-converge): an `Insert` naming a task that already has a
//! placement here. Undo of a delete makes one (`history::inverse`), so does a task moved back
//! from another file, and before 37274467 every sidecar `do` did (a delete and an insert of the
//! same task in one commit). Applied as a plain insert it gave two lines with one id, or lost a
//! peer's newer edit made on the line before it was inserted again, depending on arrival order.
//!
//! So it sets the whole line at its stamp, the way the other ops already settle by stamp:
//! - placement: a new one, like a move; the task shows at its newest placement;
//! - life: it brings a deleted task back unless a newer delete landed, and a delete older than
//!   the newest insert again leaves the task shown (`fields.rs`);
//! - content: each prefix field takes the inserted value unless a newer `SetField` set it, and
//!   the description restarts from the inserted one with only newer edits replayed on it
//!   (`TextHistory::reset`).
//!
//! A deleted task still takes edits, hidden on its newest placement, so the content an insert
//! again merges with is the same whatever order the ops came in. A child of `state.rs`.

use super::{DocState, Entry, StateError};
use crate::fields::{description_of, field_value, rewrite_prefix};
use crate::text_history::TextHistory;
use crate::textedit::apply_text_edits;
use txtodo_core::{Edit, OwnedLine};
use txtodo_model::{Field, Hlc, TaskId};

/// When a task was last deleted here and last inserted again. Merge metadata, like `stamps`.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct Life {
    died: Option<Hlc>,
    reborn: Option<Hlc>,
}

/// The fields an insert again sets, in the order `rewrite_prefix` takes them.
const PREFIX_FIELDS: [Field; 4] = [
    Field::Completed,
    Field::CompletionDate,
    Field::CreationDate,
    Field::Priority,
];

impl DocState {
    /// The slot holding `task`'s line: its shown placement, else (deleted, or moved away) its
    /// newest one, where edits still land, hidden.
    pub(crate) fn content_slot(&self, task: TaskId) -> Option<usize> {
        self.live_slot(task).or_else(|| {
            (0..self.entries.len())
                .filter(|&s| self.entries[s].id() == Some(task))
                .max_by_key(|&s| self.stamps[s])
        })
    }

    /// The line in `slot`.
    pub(crate) fn slot_line(&self, slot: usize) -> &OwnedLine {
        self.entries[slot].line()
    }

    /// Records a delete of `task` stamped `hlc`, and says whether it hides the task: not when an
    /// insert again newer than it already landed.
    pub(crate) fn note_death(&mut self, task: TaskId, hlc: Hlc) -> bool {
        let life = self.life.entry(task).or_default();
        life.died = life.died.max(Some(hlc));
        life.reborn.is_none_or(|r| r <= hlc)
    }

    /// An `Insert` of `task`, which already has a placement here (the module doc).
    pub(super) fn reinsert(
        &mut self,
        task: TaskId,
        after: Option<TaskId>,
        line: OwnedLine,
        hlc: Hlc,
    ) -> Result<(), StateError> {
        let spot = self.slot_after(after, hlc)?;
        let current = self
            .content_slot(task)
            .map(|s| self.entries[s].line().clone())
            .ok_or(StateError::UnknownTask(task))?;
        let merged = self.merged_line(task, &current, line, hlc)?;
        let was_live = self.live_slot(task).is_some();
        let at = spot.at;
        let entry = Entry::Task {
            id: task,
            line: merged.clone(),
        };
        self.insert_entry(spot, entry, hlc);
        self.hide_entry(at);
        let life = self.life.entry(task).or_default();
        life.reborn = life.reborn.max(Some(hlc));
        if was_live || life.died.is_none_or(|d| d <= hlc) {
            self.show_newest(task, at);
        }
        for entry in self.entries.iter_mut().filter(|e| e.id() == Some(task)) {
            *entry = Entry::Task {
                id: task,
                line: merged.clone(),
            };
        }
        self.rehome_onto(task, hlc);
        Ok(())
    }

    /// `inserted` with each prefix field a newer `SetField` set taken from `current`, and the
    /// description its history holds once reset to the inserted one at `hlc`.
    fn merged_line(
        &mut self,
        task: TaskId,
        current: &OwnedLine,
        inserted: OwnedLine,
        hlc: Hlc,
    ) -> Result<OwnedLine, StateError> {
        let newer = |state: &DocState, f: Field| {
            state
                .field_stamps
                .get(&(task, f))
                .is_some_and(|set| *set > hlc)
        };
        let (kept, taken): (Vec<Field>, Vec<Field>) =
            PREFIX_FIELDS.into_iter().partition(|&f| newer(self, f));
        let mut line = inserted;
        for f in kept {
            line = field_value(current, f)
                .and_then(|value| rewrite_prefix(&line, f, value))
                .ok_or(StateError::Unsupported("insert again over this line"))?;
        }
        let description = description_of(&line).ok_or(StateError::Unsupported("a blank"))?;
        let held = self
            .text_history
            .entry(task)
            .or_insert_with(|| TextHistory::new(description.clone()))
            .reset(description, hlc, apply_text_edits);
        let edit = Edit::new()
            .set_description(&held)
            .map_err(|_| StateError::Unsupported("line break"))?;
        for f in taken {
            self.record_field(task, f, hlc);
        }
        Ok(txtodo_core::apply(&line, &edit))
    }

    /// Shows `task` at its newest placement (`added`, the one just placed, on a tie).
    fn show_newest(&mut self, task: TaskId, added: usize) {
        let newest = (0..self.entries.len())
            .filter(|&s| self.entries[s].id() == Some(task))
            .max_by_key(|&s| (self.stamps[s], s == added));
        let live = self.live_slot(task);
        if newest == live {
            return;
        }
        if let Some(l) = live {
            self.hidden[l] = true;
        }
        if let Some(n) = newest {
            self.hidden[n] = false;
        }
        self.reindex();
    }

    /// After a commit: life stamps a scratch replay recorded take the commit's stamp.
    pub(super) fn settle_life(&mut self, scratch: Hlc, real: Hlc) {
        for life in self.life.values_mut() {
            for stamp in [&mut life.died, &mut life.reborn].into_iter().flatten() {
                if *stamp == scratch {
                    *stamp = real;
                }
            }
        }
    }
}
