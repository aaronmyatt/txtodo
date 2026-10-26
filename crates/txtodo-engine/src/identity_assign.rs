//! Solves the sidecar re-matching problem (design §4.1) as a minimum-cost bipartite assignment:
//! every old (already-identified) task is paired with at most one freshly-scanned line, using the
//! Hungarian algorithm (`pathfinding::kuhn_munkres_min`) to find the globally cheapest pairing
//! rather than a greedy one. A pairing costing at or above `weights.match_threshold` is rejected —
//! its task becomes a deletion, its line an insertion — per the design's stated preference for a
//! visible duplicate over a wrong merge (docs/questions.md Q2).
//!
//! `kuhn_munkres_min` compares its weights with `Ord`, which `f64` doesn't implement (no total
//! order because of NaN), so real-valued costs are scaled and rounded into `i64` first; `SCALE`
//! keeps six decimal digits of precision, far finer than `CostWeights` distinguishes. It also
//! requires a square-or-wider-than-tall matrix (rows ≤ columns), so this always pads to a square
//! `max(old, new)` matrix: `PADDING_COST` sits comfortably above any real cost (weights sum to
//! 13.5 at most), so a real pairing is always preferred to a padded one wherever one exists, and a
//! row or column stuck with padding is exactly a deletion or insertion.
//! Ref: https://docs.rs/pathfinding/latest/pathfinding/kuhn_munkres/fn.kuhn_munkres_min.html

use pathfinding::matrix::Matrix;
use pathfinding::prelude::kuhn_munkres_min;
use txtodo_model::{CostWeights, Fingerprint, TaskId};

use crate::identity_fingerprint::cost;

/// Six decimal digits of precision when converting a real-valued cost into the integer domain
/// `kuhn_munkres_min` requires.
const SCALE: f64 = 1_000_000.0;
/// Comfortably above any real cost (`CostWeights`' fields sum to 13.5 at most): a real pairing
/// always beats a padded one when a real one exists.
const PADDING_COST: f64 = 1_000.0;

/// The result of re-matching `old` fingerprints against a fresh `new` scan.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Assignment {
    /// `(task, index into `new`)` for every pairing that cleared `weights.match_threshold`.
    pub matched: Vec<(TaskId, usize)>,
    /// Old tasks with no acceptable match in `new`; their fingerprint should be retired.
    pub deleted: Vec<TaskId>,
    /// Indices into `new` with no acceptable match in `old`; each mints a fresh `TaskId`.
    pub inserted: Vec<usize>,
}

fn scaled(cost: f64) -> i64 {
    (cost * SCALE).round() as i64
}

/// Matches `old` (already-identified tasks) against `new` (this scan's fingerprints, in file
/// order) by solving the assignment problem at minimum total cost, then rejecting any pairing at
/// or above `weights.match_threshold`. `max_task_count` is the position term's normaliser
/// (`identity_fingerprint::cost`) — the *whole file's* task count, not just `old`/`new`'s own
/// lengths: a caller that pre-filters exact-content matches out of `old`/`new` before calling this
/// (`reconcile_sidecar`'s own fast path) must still pass the untrimmed count, or a moved task's
/// position delta gets normalised against a handful of leftover lines instead of the real file
/// size and can swamp every other term.
pub fn assign(
    old: &[(TaskId, Fingerprint)],
    new: &[Fingerprint],
    weights: &CostWeights,
    max_task_count: usize,
) -> Assignment {
    let n = old.len().max(new.len());
    if n == 0 {
        return Assignment {
            matched: Vec::new(),
            deleted: Vec::new(),
            inserted: Vec::new(),
        };
    }
    let padding = scaled(PADDING_COST);
    let pair_cost = |row: usize, col: usize| -> i64 {
        if row < old.len() && col < new.len() {
            scaled(cost(&old[row].1, &new[col], weights, max_task_count))
        } else {
            padding
        }
    };
    let matrix = Matrix::from_fn(n, n, |(row, col)| pair_cost(row, col));

    let (_, columns_by_row) = kuhn_munkres_min(&matrix);
    let threshold = scaled(weights.match_threshold);

    let mut matched = Vec::new();
    let mut deleted = Vec::new();
    let mut matched_new = vec![false; new.len()];

    for (row, &col) in columns_by_row.iter().enumerate() {
        // Rows at or past `old.len()` are padding rows with no task to report on.
        let Some((task, _)) = old.get(row) else {
            continue;
        };
        if col < new.len() && pair_cost(row, col) < threshold {
            matched.push((*task, col));
            matched_new[col] = true;
        } else {
            deleted.push(*task);
        }
    }

    let inserted = (0..new.len()).filter(|&i| !matched_new[i]).collect();
    Assignment {
        matched,
        deleted,
        inserted,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;
    use txtodo_model::Ulid;

    fn task(n: u128) -> TaskId {
        TaskId::new(Ulid::from_u128(n))
    }

    fn fp(description_norm: &str, line_index: usize) -> Fingerprint {
        Fingerprint {
            creation_date: None,
            projects: BTreeSet::new(),
            contexts: BTreeSet::new(),
            description_norm: description_norm.to_owned(),
            line_index,
        }
    }

    #[test]
    fn both_empty_assigns_nothing() {
        let got = assign(&[], &[], &CostWeights::DEFAULT, 0);
        assert_eq!(
            got,
            Assignment {
                matched: Vec::new(),
                deleted: Vec::new(),
                inserted: Vec::new()
            }
        );
    }

    #[test]
    fn an_unchanged_line_matches_itself() {
        let a = task(1);
        let old = [(a, fp("buy milk", 0))];
        let new = [fp("buy milk", 0)];
        let got = assign(&old, &new, &CostWeights::DEFAULT, 1);
        assert_eq!(got.matched, vec![(a, 0)]);
        assert!(got.deleted.is_empty());
        assert!(got.inserted.is_empty());
    }

    #[test]
    fn a_pure_insertion_with_no_old_tasks_is_all_inserts() {
        let new = [fp("buy milk", 0), fp("call mom", 1)];
        let got = assign(&[], &new, &CostWeights::DEFAULT, 2);
        assert!(got.matched.is_empty());
        assert!(got.deleted.is_empty());
        assert_eq!(got.inserted, vec![0, 1]);
    }

    #[test]
    fn a_pure_deletion_with_no_new_lines_is_all_deletes() {
        let a = task(1);
        let b = task(2);
        let old = [(a, fp("buy milk", 0)), (b, fp("call mom", 1))];
        let got = assign(&old, &[], &CostWeights::DEFAULT, 2);
        assert!(got.matched.is_empty());
        assert_eq!(got.deleted.len(), 2);
        assert!(got.deleted.contains(&a));
        assert!(got.deleted.contains(&b));
        assert!(got.inserted.is_empty());
    }

    #[test]
    fn a_fully_rewritten_description_becomes_delete_plus_insert_not_a_forced_match() {
        let a = task(1);
        let old = [(a, fp("buy milk", 0))];
        let new = [fp("call the dentist about a checkup", 0)];
        let got = assign(&old, &new, &CostWeights::DEFAULT, 1);
        assert!(
            got.matched.is_empty(),
            "a full rewrite must not force-match"
        );
        assert_eq!(got.deleted, vec![a]);
        assert_eq!(got.inserted, vec![0]);
    }

    #[test]
    fn the_cheaper_of_two_candidates_wins_the_match() {
        let a = task(1);
        // Old task at line 0, described "buy milk". Two new lines: one nearly identical at 0,
        // one a different task at 1. The Hungarian solver must pick the globally cheapest total
        // assignment, i.e. pair `a` with the near-identical line, not the unrelated one.
        let old = [(a, fp("buy milk", 0))];
        let new = [fp("buy oat milk", 0), fp("call the dentist", 1)];
        let got = assign(&old, &new, &CostWeights::DEFAULT, 2);
        assert_eq!(got.matched, vec![(a, 0)]);
        assert_eq!(got.inserted, vec![1]);
    }

    #[test]
    fn a_reordered_but_otherwise_identical_set_matches_by_content_not_position() {
        let a = task(1);
        let b = task(2);
        let old = [(a, fp("buy milk", 0)), (b, fp("call mom", 1))];
        // Same two tasks, order swapped in the file.
        let new = [fp("call mom", 0), fp("buy milk", 1)];
        let got = assign(&old, &new, &CostWeights::DEFAULT, 2);
        let mut matched = got.matched.clone();
        matched.sort_by_key(|(t, _)| *t);
        assert_eq!(matched, vec![(a, 1), (b, 0)]);
        assert!(got.deleted.is_empty());
        assert!(got.inserted.is_empty());
    }
}
