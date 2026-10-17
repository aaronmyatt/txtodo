//! Which blank a `BlankRemove` hides (lab lan-converge seed 435090918, task partition-converge).
//! A blank has no id, so the op names its anchor: "the blank right after task T". Which blank that
//! is depends on what else is there, and a blank or a placement of T that lands later can change
//! it. Picking the blank once, when the op lands, made two devices hide two different blanks.
//!
//! So a `BlankRemove` is kept as an eraser: a hidden slot placed like any other op (RGA, ghosts,
//! re-homed with its anchor by `state_rehome.rs`). After every op that changes the sequence, each
//! eraser, in sequence order, claims the first unclaimed blank after it that its author could
//! have seen, and every claimed blank is hidden. That is a function of the sequence alone, so
//! every device holding the same sequence shows the same blanks. A child of `state.rs`.

use std::collections::{HashMap, HashSet};

use super::{DocState, Entry};
use txtodo_model::Hlc;

impl DocState {
    /// Hides each blank an eraser claims and shows every other blank.
    pub(super) fn settle_erasers(&mut self) {
        if !self.erasers.contains(&true) {
            return;
        }
        let claimed: HashSet<usize> = self.claims().into_values().collect();
        let mut changed = false;
        for s in 0..self.entries.len() {
            if self.is_blank(s) && !self.erasers[s] {
                let hide = claimed.contains(&s);
                changed |= self.hidden[s] != hide;
                self.hidden[s] = hide;
            }
        }
        if changed {
            self.reindex();
        }
    }

    /// Each eraser's slot and the blank it claims, erasers taken in sequence order.
    pub(super) fn claims(&self) -> HashMap<usize, usize> {
        let mut claims = HashMap::new();
        let mut taken = HashSet::new();
        for s in (0..self.entries.len()).filter(|&s| self.erasers[s]) {
            if let Some(b) = self.claim(s, &taken) {
                taken.insert(b);
                claims.insert(s, b);
            }
        }
        claims
    }

    /// The blank the eraser in `eraser` claims: the first unclaimed blank after it that is not
    /// newer than the eraser, when every shown line in between was placed by a newer op than that
    /// blank (RGA puts a concurrent add between the anchor and the blank; an older line there
    /// means the author saw no such blank). Ghosts, other erasers, claimed blanks and anything
    /// newer than the eraser (its author never saw it) are passed over.
    fn claim(&self, eraser: usize, taken: &HashSet<usize>) -> Option<usize> {
        let mine = self.stamps[eraser];
        let mut oldest_between: Option<Hlc> = None;
        // Bounded by the document length.
        for s in eraser + 1..self.entries.len() {
            let passed = self.erasers[s]
                || (self.hidden[s] && !self.is_blank(s))
                || taken.contains(&s)
                || self.stamps[s] > mine;
            if passed {
                continue;
            }
            if self.is_blank(s) {
                let adjacent = oldest_between.is_none_or(|old| old > self.stamps[s]);
                return adjacent.then_some(s);
            }
            oldest_between = Some(oldest_between.map_or(self.stamps[s], |o| o.min(self.stamps[s])));
        }
        None
    }

    fn is_blank(&self, s: usize) -> bool {
        matches!(self.entries[s], Entry::Blank(_))
    }
}
