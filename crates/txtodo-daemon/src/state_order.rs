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
use txtodo_model::{DeviceId, FilePath, Hlc, Op, OpId, OpKind, Principal, TaskId, Ulid};

/// Two states are equal when they hold the same document; which op placed a line is not part of
/// the document.
impl PartialEq for DocState {
    fn eq(&self, other: &DocState) -> bool {
        self.path == other.path
            && self.entries == other.entries
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
    /// Puts `entry`, placed by an op stamped `hlc`, at `i`.
    pub(super) fn insert_entry(&mut self, i: usize, entry: Entry, hlc: Hlc) {
        self.entries.insert(i, entry);
        self.stamps.insert(i, hlc);
        debug_assert_eq!(self.stamps.len(), self.entries.len());
    }

    /// Index for an entry placed after `after` by an op stamped `hlc`: right after the anchor,
    /// then past every entry placed by a later op (and so past whatever was placed after those).
    pub(super) fn slot_after(&self, after: Option<TaskId>, hlc: Hlc) -> Result<usize, StateError> {
        let mut at = self.position_after(after)?;
        debug_assert!(at <= self.stamps.len());
        // Bounded by the document length.
        while self.stamps.get(at).is_some_and(|placed| *placed > hlc) {
            at += 1;
        }
        debug_assert!(at <= self.entries.len());
        Ok(at)
    }

    /// Whether a same-file move stamped `hlc` of the entry at `i` loses to the placement it
    /// already has: of two concurrent moves of one task, the newest wins on every device.
    pub(super) fn is_stale_move(&self, i: usize, hlc: Hlc) -> bool {
        debug_assert!(i < self.stamps.len());
        self.stamps.get(i).is_some_and(|placed| *placed > hlc)
    }

    /// After a commit: whatever a scratch replay placed takes the commit's real stamp, the one
    /// its ops carry on every other device.
    pub(crate) fn settle_scratch_stamps(&mut self, hlc: Hlc) {
        debug_assert!(hlc < scratch_stamp(), "a real stamp");
        let scratch = scratch_stamp();
        for placed in self.stamps.iter_mut().filter(|p| **p == scratch) {
            *placed = hlc;
        }
        debug_assert_eq!(self.stamps.len(), self.entries.len());
    }

    /// Takes the stamps of `replayed` (this document rebuilt from its log, what a peer holds):
    /// all of them when it holds the same lines, else each task's by id, blanks left as they are.
    pub(crate) fn adopt_stamps(&mut self, replayed: &DocState) {
        if replayed.entries == self.entries {
            self.stamps.clone_from(&replayed.stamps);
            debug_assert_eq!(self.stamps.len(), self.entries.len());
            return;
        }
        let by_task: HashMap<TaskId, Hlc> = replayed
            .entries
            .iter()
            .zip(&replayed.stamps)
            .filter_map(|(entry, placed)| Some((entry.id()?, *placed)))
            .collect();
        for (entry, placed) in self.entries.iter().zip(self.stamps.iter_mut()) {
            if let Some(stamp) = entry.id().and_then(|id| by_task.get(&id)) {
                *placed = *stamp;
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

    /// Test seam: each line's stamp, in file order.
    #[cfg(test)]
    pub(crate) fn stamps(&self) -> &[Hlc] {
        &self.stamps
    }
}
