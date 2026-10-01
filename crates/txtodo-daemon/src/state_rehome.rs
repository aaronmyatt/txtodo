//! A placement that arrives late (ADR 0033's known gap, found by the p2p lab's lan-converge seed
//! 435090918, task partition-converge). An op anchored on task T follows T's placement with the
//! newest stamp not newer than its own, but only among the placements it finds when it lands. If
//! a move of T older than that op lands after it, the op already sits under an older placement
//! of T; on a device that had the move first it sits under the move's. So when a placement of T
//! lands, every entry that should follow it and does not moves under it, with the entries placed
//! after it in turn (its block), and both devices hold one sequence.
//!
//! Each slot keeps its parent, the anchor placement its op followed, as that task's id and that
//! placement's stamp. RGA (Roh et al. 2011, §4) keeps a parent's children and their own children
//! right behind it, so a block is the run after its head of entries whose parent is in it.
//! A child of `state.rs`, like `state_order.rs`, so it keeps the slot vectors in step.

use std::collections::HashSet;

use super::DocState;
use super::order::Spot;
use txtodo_model::{Hlc, TaskId};

impl DocState {
    /// After `task`'s placement stamped `placed` landed: moves under it each block whose head
    /// follows an older placement of `task` but is newer than this one.
    pub(super) fn rehome_onto(&mut self, task: TaskId, placed: Hlc) {
        // Each pass moves one block for good (its head's parent becomes this placement); bounded
        // by the document length.
        for _ in 0..self.entries.len() {
            let Some(head) = (0..self.entries.len()).find(|&s| self.follows_older(s, task, placed))
            else {
                return;
            };
            if !self.move_block(head, task, placed) {
                return;
            }
        }
    }

    /// Whether the entry in `s` was placed after an older placement of `task` than `placed`, by
    /// an op newer than `placed`: it would follow `placed` had that arrived first. Equal stamps
    /// are one commit, applied in order, so they never move.
    fn follows_older(&self, s: usize, task: TaskId, placed: Hlc) -> bool {
        matches!(self.parents[s], Some((t, p)) if t == task && p < placed && placed < self.stamps[s])
    }

    /// Moves the block `head` starts to its spot under `task`'s placement stamped `placed`.
    /// `false`, nothing moved, when that placement is inside the block: parents are never newer
    /// than their children, so it is only possible past a pruned ghost.
    fn move_block(&mut self, head: usize, task: TaskId, placed: Hlc) -> bool {
        let end = self.block_end(head);
        let inside =
            (head..end).any(|s| self.entries[s].id() == Some(task) && self.stamps[s] == placed);
        if inside {
            return false;
        }
        let entries: Vec<_> = self.entries.drain(head..end).collect();
        let stamps: Vec<_> = self.stamps.drain(head..end).collect();
        let hidden: Vec<_> = self.hidden.drain(head..end).collect();
        let mut parents: Vec<_> = self.parents.drain(head..end).collect();
        let Ok(Spot { at, parent }) = self.slot_after(Some(task), stamps[0]) else {
            debug_assert!(false, "the placement it moves under is here");
            return false;
        };
        debug_assert_eq!(parent, Some((task, placed)));
        parents[0] = parent;
        let tail = at..at;
        self.entries.splice(tail.clone(), entries);
        self.stamps.splice(tail.clone(), stamps);
        self.hidden.splice(tail.clone(), hidden);
        self.parents.splice(tail, parents);
        self.reindex();
        true
    }

    /// One past the last slot of the block `head` starts: `head`, then each following entry whose
    /// parent is a placement already in the block.
    fn block_end(&self, head: usize) -> usize {
        let mut members: HashSet<(TaskId, Hlc)> = HashSet::new();
        let mut end = head;
        loop {
            if let Some(id) = self.entries[end].id() {
                members.insert((id, self.stamps[end]));
            }
            end += 1;
            let in_block = self
                .parents
                .get(end)
                .is_some_and(|p| p.is_some_and(|p| members.contains(&p)));
            if !in_block {
                return end;
            }
        }
    }
}
