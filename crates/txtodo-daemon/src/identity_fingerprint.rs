//! Building a `Fingerprint` from a parsed task, and the pairwise cost between two fingerprints
//! (design §4.1, docs/questions.md Q2's formula). Pure text math — no I/O, no clock — so it lives
//! here rather than routing through `state.rs`.
//!
//! `description_norm` is built from [`Task::plain_words`], not the raw description: the
//! description term is meant to be the human-legible identity signal distinct from the
//! project/context terms, and `plain_words` already excludes `+project`/`@context`/`key:value`
//! tags/URLs for exactly this kind of "meaningful content only" comparison (it's also what a
//! `ref:` slug is generated from). Using the raw description instead would double-count a
//! `+project`/`@context` change in both its own term and the description term.
// Only `identity_assign::assign` calls `cost`, and `fingerprint_of` isn't called anywhere yet —
// wired in when `state.rs`/`reconcile_sidecar.rs` land (plan `floofy-swinging-brooks.md`).
#![allow(dead_code)]

use txtodo_core::Task;
use txtodo_model::{CostWeights, Fingerprint};

use crate::identity_levenshtein::description_distance;

/// Builds the fingerprint sidecar mode matches on for `task`, found at `line_index` among task
/// lines only (blanks excluded) — see [`Fingerprint::line_index`].
pub fn fingerprint_of(task: &Task<'_>, line_index: usize) -> Fingerprint {
    Fingerprint {
        creation_date: task.creation_date.map(|d| (d.year(), d.month(), d.day())),
        projects: task.projects().map(str::to_owned).collect(),
        contexts: task.contexts().map(str::to_owned).collect(),
        description_norm: task
            .plain_words()
            .collect::<Vec<_>>()
            .join(" ")
            .to_lowercase(),
        line_index,
    }
}

/// `1 - jaccard` similarity between two sorted name sets. Two tasks that both carry no
/// `+project` (or no `@context`) are treated as agreeing (cost `0.0`), not as maximally
/// different — there is no signal to disagree on.
fn set_distance(
    a: &std::collections::BTreeSet<String>,
    b: &std::collections::BTreeSet<String>,
) -> f64 {
    if a.is_empty() && b.is_empty() {
        return 0.0;
    }
    let intersection = a.intersection(b).count() as f64;
    let union = a.union(b).count() as f64;
    1.0 - intersection / union
}

/// The re-identification cost between an old (already-identified) fingerprint and a candidate new
/// one, per docs/questions.md Q2's formula. `max_task_count` normalises the position term — the
/// total number of task lines in play for this match round (old and new combined is the caller's
/// convention; see `identity_assign::assign`).
pub fn cost(
    old: &Fingerprint,
    new: &Fingerprint,
    weights: &CostWeights,
    max_task_count: usize,
) -> f64 {
    let date_mismatch = f64::from(u8::from(old.creation_date != new.creation_date));
    let position_delta = old.line_index.abs_diff(new.line_index) as f64;
    let position_term = position_delta / (max_task_count.max(1) as f64);

    weights.date * date_mismatch
        + weights.project * set_distance(&old.projects, &new.projects)
        + weights.context * set_distance(&old.contexts, &new.contexts)
        + weights.description * description_distance(&old.description_norm, &new.description_norm)
        + weights.position * position_term
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;
    use txtodo_core::{LineKind, Mode, parse_line};

    fn task(raw: &str) -> Task<'_> {
        match parse_line(raw, Mode::Lenient).unwrap().kind {
            LineKind::Task(t) => t,
            LineKind::Blank => panic!("blank"),
        }
    }

    fn names(items: &[&str]) -> BTreeSet<String> {
        items.iter().map(|s| (*s).to_owned()).collect()
    }

    fn fp(
        creation_date: Option<(u16, u8, u8)>,
        projects: &[&str],
        contexts: &[&str],
        description_norm: &str,
        line_index: usize,
    ) -> Fingerprint {
        Fingerprint {
            creation_date,
            projects: names(projects),
            contexts: names(contexts),
            description_norm: description_norm.to_owned(),
            line_index,
        }
    }

    #[test]
    fn fingerprint_of_reads_date_projects_contexts_and_plain_words() {
        let t = task("2026-09-13 buy milk +errands @home due:2026-09-20 id:x");
        let got = fingerprint_of(&t, 3);
        assert_eq!(
            got,
            fp(Some((2026, 9, 13)), &["errands"], &["home"], "buy milk", 3)
        );
    }

    #[test]
    fn fingerprint_of_a_dateless_task_has_no_creation_date() {
        let t = task("just buy milk");
        assert_eq!(fingerprint_of(&t, 0).creation_date, None);
    }

    #[test]
    fn identical_fingerprints_cost_zero() {
        let f = fp(Some((2026, 9, 13)), &["home"], &["errand"], "buy milk", 0);
        assert_eq!(cost(&f, &f, &CostWeights::DEFAULT, 10), 0.0);
    }

    #[test]
    fn a_date_mismatch_costs_exactly_the_date_weight() {
        let a = fp(Some((2026, 9, 13)), &[], &[], "buy milk", 0);
        let b = fp(Some((2026, 9, 14)), &[], &[], "buy milk", 0);
        assert_eq!(
            cost(&a, &b, &CostWeights::DEFAULT, 10),
            CostWeights::DEFAULT.date
        );
    }

    #[test]
    fn both_sides_missing_a_project_or_context_costs_nothing_for_that_term() {
        let a = fp(None, &[], &[], "buy milk", 0);
        let b = fp(None, &[], &[], "buy milk", 5);
        let only_position = CostWeights::DEFAULT.position * (5.0 / 10.0);
        assert_eq!(cost(&a, &b, &CostWeights::DEFAULT, 10), only_position);
    }

    #[test]
    fn disjoint_project_sets_cost_the_full_project_weight() {
        let a = fp(None, &["work"], &[], "buy milk", 0);
        let b = fp(None, &["home"], &[], "buy milk", 0);
        assert_eq!(
            cost(&a, &b, &CostWeights::DEFAULT, 10),
            CostWeights::DEFAULT.project
        );
    }

    #[test]
    fn a_full_description_rewrite_alone_lands_right_at_the_threshold_boundary() {
        let a = fp(None, &[], &[], "buy milk", 0);
        let b = fp(None, &[], &[], "call the dentist", 0);
        // A full rewrite (normalized distance ~1.0) costs ~`description` (6.0), which is above
        // `match_threshold` (5.0) — exactly the case Q2 says must NOT force-match.
        assert!(cost(&a, &b, &CostWeights::DEFAULT, 10) > CostWeights::DEFAULT.match_threshold);
    }

    #[test]
    fn position_term_is_normalised_by_max_task_count() {
        let a = fp(None, &[], &[], "x", 0);
        let b = fp(None, &[], &[], "x", 4);
        assert_eq!(
            cost(&a, &b, &CostWeights::DEFAULT, 8),
            CostWeights::DEFAULT.position * 0.5
        );
    }
}
