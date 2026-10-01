//! Ghost entries (task partition-converge, ADR 0033): a deleted task, a removed blank, and the
//! spot a moved task left stay in the sequence, hidden, with the stamp that placed them. An op
//! anchored on them still lands where its author put it, on every device, whatever order the ops
//! arrive in. A child of `state.rs`, like `state_order.rs`, so it keeps `entries`, `stamps`,
//! `hidden` and `visible` in step.
//!
//! Everything a caller sees by position is the visible view (`visible`, the slots not hidden):
//! line numbers, `len`, `entry_at`, `index_of`, the bytes. A *slot* is a position in the whole
//! sequence, ghosts included; only this module, `state_order.rs` and `fields.rs` use slots.

use super::{DocState, Entry, StateError};
use txtodo_core::{File, OwnedLine};
use txtodo_model::{Hlc, TaskId};

/// Most ghosts one document keeps. Past it the oldest go: an op anchored on one of those skips,
/// as every anchor on a deleted line did before ghosts.
pub const MAX_GHOSTS_PER_FILE: usize = 10_000;

impl DocState {
    /// Number of lines, blanks included.
    pub fn len(&self) -> usize {
        self.visible.len()
    }

    /// True when the document has no lines.
    pub fn is_empty(&self) -> bool {
        self.visible.is_empty()
    }

    /// The line at `i`, by value: the backing store is not a slice after M4.
    pub fn entry_at(&self, i: usize) -> Option<Entry> {
        self.visible.get(i).map(|&s| self.entries[s].clone())
    }

    /// The line bytes of a task, if it is in the document.
    pub fn line_of(&self, id: TaskId) -> Option<OwnedLine> {
        let slot = self.live_slot(id)?;
        Some(self.entries[slot].line().clone())
    }

    /// The nearest task id before line `i` — the `after` anchor an op needs (`i == len()`: the
    /// last).
    pub fn task_before(&self, i: usize) -> Option<TaskId> {
        debug_assert!(i <= self.visible.len(), "task_before index {i} in range");
        self.visible[..i.min(self.visible.len())]
            .iter()
            .rev()
            .find_map(|&s| self.entries[s].id())
    }

    /// The line a task is on, if it is in the document.
    pub fn index_of(&self, id: TaskId) -> Option<usize> {
        self.visible
            .iter()
            .position(|&s| self.entries[s].id() == Some(id))
    }

    /// Every task's id and line, in file order (blanks skipped): item `i` is task-line index `i`.
    pub fn task_lines(&self) -> impl Iterator<Item = (TaskId, &OwnedLine)> {
        self.visible_entries()
            .filter_map(|e| Some((e.id()?, e.line())))
    }

    /// Every line with its 0-based index, blanks included (`duplicates.rs` needs line numbers).
    pub(crate) fn indexed_entries(&self) -> impl Iterator<Item = (usize, &Entry)> {
        self.visible_entries().enumerate()
    }

    /// Every line's task id in file order, `None` for a blank line (`GetFile`'s `task_ids`).
    pub fn line_ids(&self) -> impl Iterator<Item = Option<TaskId>> + '_ {
        self.visible_entries().map(Entry::id)
    }

    /// The file as bytes, byte-faithful to what `from_file` read plus the applied ops.
    pub fn to_bytes(&self) -> Vec<u8> {
        let file = File {
            lines: self.visible_entries().map(|e| e.line().clone()).collect(),
            bom: self.bom,
            ending: self.ending,
            trailing_newline: self.trailing_newline,
        };
        debug_assert_eq!(file.lines.len(), self.visible.len());
        file.to_bytes()
    }

    pub(super) fn visible_entries(&self) -> impl Iterator<Item = &Entry> {
        self.visible.iter().map(|&s| &self.entries[s])
    }

    /// The slot of a task's live (shown) placement, if it is in the document.
    pub(crate) fn live_slot(&self, id: TaskId) -> Option<usize> {
        (0..self.entries.len()).find(|&s| !self.hidden[s] && self.entries[s].id() == Some(id))
    }

    /// Whether `id` has any placement here, shown or not.
    pub(crate) fn has_placement(&self, id: TaskId) -> bool {
        self.entries.iter().any(|e| e.id() == Some(id))
    }

    /// Replaces the entry in `slot` (fields.rs rewrites lines in place).
    pub(crate) fn replace_entry(&mut self, slot: usize, entry: Entry) {
        debug_assert!(
            slot < self.entries.len(),
            "replace_entry slot {slot} in range"
        );
        self.entries[slot] = entry;
    }

    /// Hides the entry in `slot` where it is: a deleted task, a removed blank, a spot left.
    pub(crate) fn hide_entry(&mut self, slot: usize) {
        debug_assert!(!self.hidden[slot], "hide_entry: slot {slot} is shown");
        self.hidden[slot] = true;
        self.reindex();
    }

    /// Rebuilds the visible view after the sequence changed.
    pub(super) fn reindex(&mut self) {
        debug_assert_eq!(self.hidden.len(), self.entries.len());
        debug_assert_eq!(self.stamps.len(), self.entries.len());
        self.visible = (0..self.entries.len())
            .filter(|&s| !self.hidden[s])
            .collect();
    }

    /// The slot an op anchored after `after` and stamped `hlc` follows: the anchor's placement
    /// with the newest stamp not newer than the op (shown or a ghost), else its shown one, else
    /// its oldest. `None` is the top of the document.
    pub(super) fn anchor_slot(
        &self,
        after: Option<TaskId>,
        hlc: Hlc,
    ) -> Result<Option<usize>, StateError> {
        let Some(id) = after else {
            return Ok(None);
        };
        let placements: Vec<usize> = (0..self.entries.len())
            .filter(|&s| self.entries[s].id() == Some(id))
            .collect();
        // On a tie (one commit, or a hydration replay, stamps every op alike) the shown placement
        // wins: within one commit the ops apply in order, and the latest placement is the shown one.
        let not_newer = placements
            .iter()
            .copied()
            .filter(|&s| self.stamps[s] <= hlc)
            .max_by_key(|&s| (self.stamps[s], !self.hidden[s]));
        let shown = placements.iter().copied().find(|&s| !self.hidden[s]);
        let oldest = placements.iter().copied().min_by_key(|&s| self.stamps[s]);
        not_newer
            .or(shown)
            .or(oldest)
            .map(Some)
            .ok_or(StateError::UnknownTask(id))
    }

    /// Adds a placement of `task` after `after`, stamped `hlc`, as a ghost: a move that lost to a
    /// newer one, or a move of a task already deleted. A device that applied that move first
    /// holds the same spot as a ghost once the newer op hides it.
    pub(super) fn place_ghost(
        &mut self,
        task: TaskId,
        after: Option<TaskId>,
        hlc: Hlc,
    ) -> Result<(), StateError> {
        let line = self
            .entries
            .iter()
            .find(|e| e.id() == Some(task))
            .map(|e| e.line().clone())
            .ok_or(StateError::UnknownTask(task))?;
        let at = self.slot_after(after, hlc)?;
        self.insert_entry(at, Entry::Task { id: task, line }, hlc);
        self.hide_entry(at);
        Ok(())
    }

    /// Drops the oldest ghosts past [`MAX_GHOSTS_PER_FILE`].
    pub(super) fn prune_ghosts(&mut self) {
        self.prune_ghosts_to(MAX_GHOSTS_PER_FILE);
    }

    /// Drops the oldest ghosts past `max` (a parameter so a test need not make 10 000).
    pub(crate) fn prune_ghosts_to(&mut self, max: usize) {
        let ghosts = self.entries.len() - self.visible.len();
        if ghosts <= max {
            return;
        }
        let mut hidden: Vec<usize> = (0..self.entries.len())
            .filter(|&s| self.hidden[s])
            .collect();
        hidden.sort_by_key(|&s| self.stamps[s]);
        let mut drop: Vec<usize> = hidden[..ghosts - max].to_vec();
        drop.sort_unstable_by(|a, b| b.cmp(a));
        for s in drop {
            self.entries.remove(s);
            self.stamps.remove(s);
            self.hidden.remove(s);
        }
        self.reindex();
        debug_assert_eq!(self.entries.len() - self.visible.len(), max);
    }

    /// Takes the replay's whole sequence, ghosts included, when it shows the same lines by id
    /// (blanks as blanks); the text of each shown line stays this state's own. `false` when the
    /// lines differ.
    pub(super) fn adopt_sequence(&mut self, replayed: &DocState) -> bool {
        if !self.line_ids().eq(replayed.line_ids()) {
            return false;
        }
        let mut entries = replayed.entries.clone();
        for (&theirs, &mine) in replayed.visible.iter().zip(&self.visible) {
            entries[theirs] = self.entries[mine].clone();
        }
        self.entries = entries;
        self.stamps.clone_from(&replayed.stamps);
        self.hidden.clone_from(&replayed.hidden);
        self.reindex();
        true
    }
}
