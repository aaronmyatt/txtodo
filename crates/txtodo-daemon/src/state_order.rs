//! Where a placement lands when two devices place after the same line at once (task insert-order,
//! `tasks/insert-order/notes.md`). A child of `state.rs`, so it sees `entries` and `stamps`.
//!
//! Each entry keeps the HLC of the op that placed it (`Insert`, same-file `Move`, `BlankInsert`);
//! a line read from disk has [`unplaced`]. RGA's rule (Roh et al. 2011, "Replicated abstract data
//! types", §4) then orders concurrent placements the same way on every device: an op placing
//! after anchor A goes right after A and then past every following entry placed by a *later* op.
//! A local op is always the newest, so it lands right after its anchor as it always did; only a
//! peer's op that was made concurrently with, or before, what is already there moves along.
//! Entries with the op's own stamp are never skipped: one stamp is one commit of one device,
//! which every device applies in the same order.

use std::collections::HashMap;

use super::{DocState, Entry, StateError};
use crate::text_history::{MAX_EDITS_PER_TASK, TextHistory};
use crate::textedit::apply_text_edits;
use txtodo_model::{
    DeviceId, Field, FilePath, Hlc, Op, OpId, OpKind, Principal, TaskId, TextEdit, Ulid,
};

/// Two states are equal when they hold the same document; which op placed a line, and the ghosts
/// (ADR 0033), are not part of the document.
impl PartialEq for DocState {
    fn eq(&self, other: &DocState) -> bool {
        self.path == other.path
            && self.visible_entries().eq(other.visible_entries())
            && self.bom == other.bom
            && self.ending == other.ending
            && self.trailing_newline == other.trailing_newline
            && self.mode == other.mode
    }
}

impl Eq for DocState {}

/// A peer's move lost to a newer placement of the same task. Expected when two devices move one
/// line at once: debug, not warn. <https://docs.rs/tracing/latest/tracing/macro.debug.html>
pub(super) fn log_stale_move(task: TaskId) {
    tracing::debug!(%task, "move_older_than_placement");
}

/// The stamp of a line nothing placed (read from disk): older than every op.
pub(crate) fn unplaced() -> Hlc {
    Hlc::zero(DeviceId::new(Ulid::from_u128(0)))
}

/// The anchor placement an entry's op followed, as that task's id and that placement's stamp:
/// its parent in RGA's tree (`state_rehome.rs`). `None` for the top, or a line read from disk.
pub(super) type Parent = Option<(TaskId, Hlc)>;

/// Where an op places its entry: the slot, and the placement it follows.
pub(super) struct Spot {
    pub(super) at: usize,
    pub(super) parent: Parent,
}

/// The stamp scratch replays place under: newer than every real op, so a scratch op lands where
/// the same op will once it is stamped with a fresh tick. `DocState::settle_scratch_stamps`
/// swaps it for that tick when the state is committed.
fn scratch_stamp() -> Hlc {
    Hlc {
        wall_ms: u64::MAX,
        counter: u16::MAX,
        device: DeviceId::new(Ulid::from_u128(u128::MAX)),
    }
}

/// An op that is never stored or sent: the reconciler's scratch replays and the `apply_kind` test
/// seam. It places as the newest op would.
pub(crate) fn scratch_op(path: &FilePath, kind: OpKind) -> Op {
    let zero = DeviceId::new(Ulid::from_u128(0));
    let op = Op {
        id: OpId::new(Ulid::from_u128(0)),
        hlc: scratch_stamp(),
        principal: Principal::External { device: zero },
        file: path.clone(),
        kind,
    };
    debug_assert_eq!(op.hlc, scratch_stamp());
    debug_assert_eq!(&op.file, path);
    op
}

impl DocState {
    /// Puts `entry`, placed by an op stamped `hlc`, at `spot`.
    pub(super) fn insert_entry(&mut self, spot: Spot, entry: Entry, hlc: Hlc) {
        self.entries.insert(spot.at, entry);
        self.stamps.insert(spot.at, hlc);
        self.hidden.insert(spot.at, false);
        self.parents.insert(spot.at, spot.parent);
        self.erasers.insert(spot.at, false);
        self.reindex();
    }

    /// Spot for an entry placed after `after` by an op stamped `hlc`: right after the anchor's
    /// placement the op means (`anchor_slot`, ADR 0033), then past every entry placed by a later
    /// op, ghosts included (and so past whatever was placed after those).
    pub(super) fn slot_after(&self, after: Option<TaskId>, hlc: Hlc) -> Result<Spot, StateError> {
        let anchor = self.anchor_slot(after, hlc)?;
        let parent = anchor.and_then(|s| Some((self.entries[s].id()?, self.stamps[s])));
        let mut at = anchor.map_or(0, |s| s + 1);
        debug_assert!(at <= self.stamps.len());
        // Bounded by the document length.
        while self.stamps.get(at).is_some_and(|placed| *placed > hlc) {
            at += 1;
        }
        debug_assert!(at <= self.entries.len());
        Ok(Spot { at, parent })
    }

    /// Whether a same-file move stamped `hlc` of the entry at `i` loses to the placement it
    /// already has: of two concurrent moves of one task, the newest wins on every device.
    pub(super) fn is_stale_move(&self, i: usize, hlc: Hlc) -> bool {
        debug_assert!(i < self.stamps.len());
        self.stamps.get(i).is_some_and(|placed| *placed > hlc)
    }

    /// Whether a `SetField` of `task`'s `field` stamped `hlc` is older than the one the field
    /// already took. Equal is not stale: one stamp is one commit, applied in sequence.
    pub(crate) fn is_stale_field(&self, task: TaskId, field: Field, hlc: Hlc) -> bool {
        self.field_stamps
            .get(&(task, field))
            .is_some_and(|set| *set > hlc)
    }

    /// Records that `task`'s `field` took a `SetField` stamped `hlc`.
    pub(crate) fn record_field(&mut self, task: TaskId, field: Field, hlc: Hlc) {
        self.field_stamps.insert((task, field), hlc);
    }

    /// Drops `task`'s description history: its description changed some other way, so the edits
    /// kept no longer rebuild it. (A priority moved into `pri:` is kept instead:
    /// [`DocState::append_to_description`].)
    pub(crate) fn forget_text(&mut self, task: TaskId) {
        self.text_history.remove(&task);
    }

    /// A copy of `task`'s description history, to put back with [`DocState::restore_text`] when
    /// an edit that recorded into it is refused after all.
    pub(crate) fn text_history_of(&self, task: TaskId) -> Option<TextHistory> {
        self.text_history.get(&task).cloned()
    }

    pub(crate) fn restore_text(&mut self, task: TaskId, saved: Option<TextHistory>) {
        match saved {
            Some(history) => self.text_history.insert(task, history),
            None => self.text_history.remove(&task),
        };
    }

    /// `edits` stamped `hlc` on `task`'s description, which is `current` now: the description to
    /// hold, rebuilt in stamp order when they arrive late (ADR 0034).
    pub(crate) fn edit_description(
        &mut self,
        task: TaskId,
        current: &str,
        hlc: Hlc,
        edits: &[TextEdit],
    ) -> Result<String, crate::textedit::TextEditError> {
        self.text_history
            .entry(task)
            .or_insert_with(|| TextHistory::new(current.to_owned()))
            .apply(current, hlc, edits, (apply_text_edits, MAX_EDITS_PER_TASK))
    }

    /// `suffix` appended to `task`'s description, which is `current` now, by an op stamped `hlc`
    /// (a priority moved into `pri:`): the description to hold, kept in the history so a late
    /// edit still rebuilds in stamp order (`text_history.rs`).
    pub(crate) fn append_to_description(
        &mut self,
        task: TaskId,
        current: &str,
        hlc: Hlc,
        suffix: &str,
    ) -> String {
        self.text_history
            .entry(task)
            .or_insert_with(|| TextHistory::new(current.to_owned()))
            .append(current, hlc, suffix, (apply_text_edits, MAX_EDITS_PER_TASK))
    }

    /// After a commit: whatever a scratch replay placed takes the commit's real stamp, the one
    /// its ops carry on every other device.
    pub(crate) fn settle_scratch_stamps(&mut self, hlc: Hlc) {
        debug_assert!(hlc < scratch_stamp(), "a real stamp");
        let scratch = scratch_stamp();
        for placed in self.stamps.iter_mut().filter(|p| **p == scratch) {
            *placed = hlc;
        }
        for (_, placed) in self.parents.iter_mut().flatten() {
            if *placed == scratch {
                *placed = hlc;
            }
        }
        for set in self.field_stamps.values_mut().filter(|s| **s == scratch) {
            *set = hlc;
        }
        for history in self.text_history.values_mut() {
            history.settle(scratch, hlc);
        }
        self.settle_life(scratch, hlc);
        debug_assert_eq!(self.stamps.len(), self.entries.len());
    }

    /// Takes the stamps of `replayed` (this document rebuilt from its log, what a peer holds):
    /// its whole sequence, ghosts included, when it shows the same lines by id (ADR 0033: ghosts
    /// are rebuilt from the log at open); else each task's stamp by id, blanks left as they are.
    pub(crate) fn adopt_stamps(&mut self, replayed: &DocState) {
        // By task id, so they hold whatever lines moved: a field's stamp is the task's, not a spot's.
        self.field_stamps.clone_from(&replayed.field_stamps);
        self.text_history.clone_from(&replayed.text_history);
        self.life.clone_from(&replayed.life);
        if self.adopt_sequence(replayed) {
            return;
        }
        let by_task: HashMap<TaskId, (Hlc, Parent)> = replayed
            .visible
            .iter()
            .filter_map(|&s| {
                let id = replayed.entries[s].id()?;
                Some((id, (replayed.stamps[s], replayed.parents[s])))
            })
            .collect();
        for (s, entry) in self.entries.iter().enumerate() {
            if let Some(&(stamp, parent)) = entry.id().and_then(|id| by_task.get(&id)) {
                self.stamps[s] = stamp;
                self.parents[s] = parent;
            }
        }
        debug_assert_eq!(self.stamps.len(), self.entries.len());
    }

    /// The newest stamp any entry carries: the actor's clock must be past it, or its own next op
    /// would sort as older than a line already here.
    pub(crate) fn newest_stamp(&self) -> Option<Hlc> {
        self.stamps
            .iter()
            .copied()
            .filter(|p| *p != unplaced())
            .max()
    }

    /// Test seam: each shown line's stamp, in file order.
    #[cfg(test)]
    pub(crate) fn stamps(&self) -> Vec<Hlc> {
        self.visible.iter().map(|&s| self.stamps[s]).collect()
    }
}
