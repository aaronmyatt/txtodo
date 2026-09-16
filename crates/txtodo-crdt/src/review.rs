//! Same-word edit detection after an import (plan M4, tasks/crdt-needs-review). Concurrency comes
//! from Loro's causal history, never from the HLC: an `Hlc` is a total order, so it cannot say
//! whether two edits saw each other. With the frontiers an [`crate::Imported`] names, the local
//! edits are `diff(ancestor → before)` and the peer's are `diff(ancestor → remote)`; both are
//! expressed against the ancestor text, so two ranges that overlap or touch mean two people
//! rewrote the same word without seeing each other. Adjacent counts: `foo|bar` interleaved from
//! two sides reads as garbage even with no shared character.

use std::collections::{BTreeMap, HashMap};

use loro::{ContainerID, TextDelta, event::Diff};
use txtodo_model::{FilePath, TaskId};

use crate::doc::LoroDocument;
use crate::doc::sync::Imported;

/// Most flags one file may raise from one import; past this the file is flagged once instead.
pub const MAX_REVIEW_FLAGS_PER_FILE: usize = 64;

/// One task two sides rewrote in the same place.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReviewFlag {
    /// The task.
    pub task: TaskId,
    /// Its file.
    pub file: FilePath,
    /// The description as this device had it before the import.
    pub mine: String,
    /// The description as the peer had it.
    pub theirs: String,
}

/// What an import's review found.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Review {
    /// Per-task flags, at most `MAX_REVIEW_FLAGS_PER_FILE` per file.
    pub flags: Vec<ReviewFlag>,
    /// Files that would have exceeded the cap; flagged once, whole, instead.
    pub overflowed: Vec<FilePath>,
}

/// Why a review could not run.
#[derive(Debug)]
pub enum ReviewError {
    /// A frontier the import named is not in the document.
    Loro(loro::LoroError),
    /// A text container belongs to no task (a shape this crate did not write).
    Orphan(ContainerID),
}

impl std::fmt::Display for ReviewError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ReviewError::Loro(e) => write!(f, "loro: {e}"),
            ReviewError::Orphan(c) => write!(f, "text container {c} belongs to no task"),
        }
    }
}

impl std::error::Error for ReviewError {}

/// A char range in ancestor coordinates, inclusive of its insertion point.
type Range = (usize, usize);

/// Detects same-word concurrent edits for the import described by `imported`. Thin wrapper around
/// `detect_inner` for the tracing span (`#[instrument]` on the real body overflows the
/// `cognitive_complexity` budget).
#[tracing::instrument(skip_all)]
pub fn detect(doc: &LoroDocument, imported: &Imported) -> Result<Review, ReviewError> {
    let review = detect_inner(doc, imported)?;
    log_review_detected(&review);
    Ok(review)
}

fn detect_inner(doc: &LoroDocument, imported: &Imported) -> Result<Review, ReviewError> {
    if !imported.applied {
        return Ok(Review::default());
    }
    let mine = edited_ranges(doc, &imported.ancestor, &imported.before)?;
    let theirs = edited_ranges(doc, &imported.ancestor, &imported.remote)?;
    let mut hits: Vec<ContainerID> = mine
        .iter()
        .filter(|(cid, ours)| theirs.get(*cid).is_some_and(|rs| touches(ours, rs)))
        .map(|(cid, _)| cid.clone())
        .collect();
    hits.sort_by_key(ToString::to_string);
    if hits.is_empty() {
        return Ok(Review::default());
    }
    let ours = doc.at(&imported.before).map_err(ReviewError::Loro)?;
    let peers = doc.at(&imported.remote).map_err(ReviewError::Loro)?;
    let mut per_file: BTreeMap<FilePath, Vec<ReviewFlag>> = BTreeMap::new();
    // Bounded by the number of text containers the diff touched.
    for cid in hits {
        let task = doc
            .task_of_container(&cid)
            .ok_or_else(|| ReviewError::Orphan(cid.clone()))?;
        let Some(file) = doc.file_of_task(task) else {
            continue; // a task in no list (deleted on both sides) is nobody's conflict
        };
        per_file.entry(file.clone()).or_default().push(ReviewFlag {
            task,
            file,
            mine: ours.description(task).unwrap_or_default(),
            theirs: peers.description(task).unwrap_or_default(),
        });
    }
    let review = cap_per_file(per_file);
    debug_assert!(
        review
            .flags
            .iter()
            .all(|f| f.mine != f.theirs || f.mine.is_empty())
    );
    Ok(review)
}

/// `detect`'s own outcome, once the diff walk has landed: counts only — never a `ReviewFlag`'s
/// `task`/`file`/`mine`/`theirs`, which is task content.
fn log_review_detected(review: &Review) {
    tracing::debug!(
        flags = review.flags.len(),
        overflowed = review.overflowed.len(),
        "crdt_review_detected"
    );
}

/// Applies `MAX_REVIEW_FLAGS_PER_FILE`: an over-cap file keeps no per-task flags.
fn cap_per_file(per_file: BTreeMap<FilePath, Vec<ReviewFlag>>) -> Review {
    let mut review = Review::default();
    for (file, flags) in per_file {
        if flags.len() > MAX_REVIEW_FLAGS_PER_FILE {
            review.overflowed.push(file);
        } else {
            review.flags.extend(flags);
        }
    }
    debug_assert!(
        review
            .flags
            .iter()
            .all(|f| !review.overflowed.contains(&f.file))
    );
    debug_assert!(review.flags.len() <= MAX_REVIEW_FLAGS_PER_FILE * (review.flags.len().max(1)));
    review
}

/// The char ranges each description text changed between two frontiers, per text container.
fn edited_ranges(
    doc: &LoroDocument,
    from: &loro::Frontiers,
    to: &loro::Frontiers,
) -> Result<HashMap<ContainerID, Vec<Range>>, ReviewError> {
    let batch = doc.diff(from, to).map_err(ReviewError::Loro)?;
    let mut out = HashMap::new();
    for (cid, diff) in batch.iter() {
        if let Diff::Text(deltas) = diff {
            out.insert(cid.clone(), ranges_of(deltas));
        }
    }
    Ok(out)
}

/// Walks a delta: a retain moves the cursor, an insert marks its point, a delete marks its span.
fn ranges_of(deltas: &[TextDelta]) -> Vec<Range> {
    let mut pos = 0usize;
    let mut ranges = Vec::new();
    for d in deltas {
        match d {
            TextDelta::Retain { retain, .. } => pos += retain,
            TextDelta::Insert { .. } => ranges.push((pos, pos)),
            TextDelta::Delete { delete } => {
                ranges.push((pos, pos + delete));
                pos += delete;
            }
        }
    }
    debug_assert!(ranges.iter().all(|(a, b)| a <= b));
    debug_assert!(
        ranges.windows(2).all(|w| w[0].1 <= w[1].0),
        "ranges advance"
    );
    ranges
}

/// True when any pair of ranges overlaps or is adjacent.
fn touches(a: &[Range], b: &[Range]) -> bool {
    a.iter()
        .any(|(a0, a1)| b.iter().any(|(b0, b1)| a0 <= b1 && b0 <= a1))
}

#[cfg(test)]
mod unit {
    use super::*;

    #[test]
    fn touches_is_overlap_or_adjacency_and_ranges_follow_the_delta() {
        assert!(touches(&[(4, 9)], &[(9, 12)]), "adjacent counts");
        assert!(touches(&[(4, 9)], &[(6, 6)]), "an insertion inside");
        assert!(!touches(&[(4, 9)], &[(10, 12)]));
        assert!(!touches(&[], &[(0, 0)]));
        let deltas = [
            TextDelta::Retain {
                retain: 4,
                attributes: None,
            },
            TextDelta::Delete { delete: 5 },
            TextDelta::Insert {
                insert: "geese".into(),
                attributes: None,
            },
        ];
        assert_eq!(ranges_of(&deltas), vec![(4, 9), (9, 9)]);
    }
}
