//! Sidecar's file → ops (design §4.1, docs/questions.md Q2): the same job as `reconcile()`, but
//! for a workspace with no `id:` tags to key on. Where `reconcile()` diffs `old` and `new` by
//! content/id (`txtodo_core::diff_lines`, frozen since M1) and then reads ids straight off the
//! text, this instead re-identifies tasks by solving the fingerprint assignment problem
//! (`identity_assign::assign`), builds its own `Vec<LineDiff>` from the result, and feeds that
//! into the *same* four passes `reconcile()` uses (`delete_pass`/`change_pass`/`task_pass`/
//! `blank_pass`) — sidecar mode is a different way to decide the diff, not a different set of ops
//! once the diff exists.
//!
//! Two things `txtodo_core::diff_lines` gets from a byte/id-level Myers diff have no fingerprint
//! equivalent, so they're rebuilt here directly:
//! - **Move vs. Change**: a matched pair (old task, new line) is `Change` when its position is
//!   consistent with the majority of other matches, `Move` otherwise — the same distinction
//!   `pair_moves` draws for `Key::Id` lines, decided here by a longest-increasing-subsequence over
//!   matched pairs sorted by their old position (`lis_indices`). A pair that is both moved and
//!   edited only gets the `Move`, exactly mirroring `pair_moves`, which also drops a same-id
//!   pair's content diff once it's classified as a move — not fixed here, since fixing it would
//!   break parity with tagged mode.
//! - **Blank lines**: fingerprints don't cover blanks at all, so each blank is anchored to the
//!   nearest preceding task (`prev_task`, already used by tagged mode's own `blank_pass`) and
//!   paired positionally within that run: the first `min(old, new)` blanks in a run are left
//!   alone, any excess becomes a `Delete` (old) or `Insert` (new) — "run-based positional
//!   pairing".

use std::collections::{BTreeMap, BTreeSet};

use txtodo_core::{File, LineDiff};
use txtodo_model::{CostWeights, FilePath, Fingerprint, TaskId};

use crate::identity_assign::assign;
use crate::identity_fingerprint::fingerprint_of;
use crate::reconcile::{
    Reconciled, blank_pass, change_pass, delete_pass, prev_task, task_of, task_pass,
};

/// A file's bytes and its lines' task ids together — sidecar has no `id:` tag in the text to read
/// them from (unlike tagged mode's `fast_id_of`), so the caller supplies them alongside.
#[derive(Clone, Copy)]
pub struct Side<'a> {
    /// The file.
    pub file: &'a File,
    /// Each line's task id, `None` for a blank — aligned to `file.lines`.
    pub ids: &'a [Option<TaskId>],
}

/// Derives ops that turn `old` (our last projection) into `new` (the bytes on disk),
/// re-identifying tasks by fingerprint instead of by tag.
pub fn reconcile_sidecar(
    old: Side<'_>,
    new: &File,
    path: &FilePath,
    weights: &CostWeights,
    mint: &mut dyn FnMut() -> TaskId,
) -> Reconciled {
    let OldFingerprints { pairs, info } = old_fingerprints(old);
    let (new_fps, new_task_positions) = new_fingerprints(new);
    let assignment = assign(&pairs, &new_fps, weights);

    let mut new_ids: Vec<Option<TaskId>> = vec![None; new.lines.len()];
    for &(task, new_tli) in &assignment.matched {
        new_ids[new_task_positions[new_tli]] = Some(task);
    }
    let mut minted = 0usize;
    for &new_tli in &assignment.inserted {
        new_ids[new_task_positions[new_tli]] = Some(mint());
        minted += 1;
    }
    let reused = assignment.matched.len();

    let mut deletes: Vec<usize> = assignment.deleted.iter().map(|task| info[task].1).collect();
    let mut inserts: Vec<usize> = assignment
        .inserted
        .iter()
        .map(|&tli| new_task_positions[tli])
        .collect();
    let Classified { changes, moves } = classify_matched(
        &assignment.matched,
        &info,
        &new_task_positions,
        old.file,
        new,
    );
    let new_side = Side {
        file: new,
        ids: &new_ids,
    };
    pair_blank_runs(old, new_side, &mut deletes, &mut inserts);

    deletes.sort_unstable();
    inserts.sort_unstable();

    let mut diffs: Vec<LineDiff> = Vec::new();
    diffs.extend(deletes.iter().map(|&from| LineDiff::Delete { from }));
    diffs.extend(
        changes
            .iter()
            .map(|&(from, to)| LineDiff::Change { from, to }),
    );
    diffs.extend(moves.iter().map(|&(from, to)| LineDiff::Move { from, to }));
    diffs.extend(inserts.iter().map(|&to| LineDiff::Insert { to }));

    let mut ops = Vec::new();
    delete_pass(&mut ops, &diffs, old.file, old.ids);
    change_pass(&mut ops, &diffs, old.file, old.ids, new);
    task_pass(&mut ops, &diffs, new, &new_ids, path);
    blank_pass(&mut ops, &diffs, &new_ids);

    Reconciled {
        ops,
        file: new.clone(),
        minted,
        reused,
        ids: new_ids,
    }
}

/// `old`'s task-line fingerprints, in file order, and where each task actually lives.
struct OldFingerprints {
    /// `(task, fingerprint)` pairs in file order — that order doubles as each task's "old
    /// position" for `classify_matched`'s LIS.
    pairs: Vec<(TaskId, Fingerprint)>,
    /// `task -> (old position, raw row in `old.lines`)`.
    info: BTreeMap<TaskId, (usize, usize)>,
}

fn old_fingerprints(old: Side<'_>) -> OldFingerprints {
    let mut pairs = Vec::new();
    let mut info = BTreeMap::new();
    for (raw, line) in old.file.lines.iter().enumerate() {
        let Some(t) = task_of(line) else { continue };
        debug_assert!(
            old.ids[raw].is_some(),
            "sidecar mode: every old task line has a resolved id"
        );
        let Some(task) = old.ids[raw] else { continue };
        let position = pairs.len();
        pairs.push((task, fingerprint_of(&t, position)));
        info.insert(task, (position, raw));
    }
    OldFingerprints { pairs, info }
}

/// `new`'s task-line fingerprints, in file order, plus each one's raw row (`new_task_positions`).
fn new_fingerprints(new: &File) -> (Vec<Fingerprint>, Vec<usize>) {
    let mut fps = Vec::new();
    let mut positions = Vec::new();
    for (raw, line) in new.lines.iter().enumerate() {
        let Some(t) = task_of(line) else { continue };
        fps.push(fingerprint_of(&t, fps.len()));
        positions.push(raw);
    }
    (fps, positions)
}

/// Raw-row pairs for a matched-pair pass, split by whether the pair stayed in relative order.
struct Classified {
    /// In the longest increasing subsequence and with bytes that actually differ.
    changes: Vec<(usize, usize)>,
    /// Outside the longest increasing subsequence — moved, regardless of content.
    moves: Vec<(usize, usize)>,
}

/// Splits `matched` into changes and moves: a pair that stays in file-order relative to the
/// others (the longest increasing subsequence of new positions, sorted by old position) is a
/// change when its bytes actually differ and dropped entirely when they don't (a `Keep`, which
/// every pass below treats as a no-op); everything outside the LIS is a move.
fn classify_matched(
    matched: &[(TaskId, usize)],
    old_info: &BTreeMap<TaskId, (usize, usize)>,
    new_task_positions: &[usize],
    old: &File,
    new: &File,
) -> Classified {
    let mut ordered = matched.to_vec();
    ordered.sort_unstable_by_key(|(task, _)| old_info[task].0);
    let seq: Vec<usize> = ordered.iter().map(|&(_, new_tli)| new_tli).collect();
    let lis: BTreeSet<usize> = lis_indices(&seq).into_iter().collect();

    let mut changes = Vec::new();
    let mut moves = Vec::new();
    for (i, &(task, new_tli)) in ordered.iter().enumerate() {
        let old_row = old_info[&task].1;
        let new_row = new_task_positions[new_tli];
        if lis.contains(&i) {
            if old.lines[old_row].bytes() != new.lines[new_row].bytes() {
                changes.push((old_row, new_row));
            }
        } else {
            moves.push((old_row, new_row));
        }
    }
    Classified { changes, moves }
}

/// Appends run-based blank deletes/inserts to `deletes`/`inserts`: every blank is keyed by the
/// task immediately above it (`None` for a run before the first task), and within each key the
/// first `min(old, new)` blanks are left alone — only the excess on either side is a diff.
fn pair_blank_runs(
    old: Side<'_>,
    new: Side<'_>,
    deletes: &mut Vec<usize>,
    inserts: &mut Vec<usize>,
) {
    let mut old_slots: BTreeMap<Option<TaskId>, Vec<usize>> = BTreeMap::new();
    for raw in 0..old.file.lines.len() {
        if old.ids[raw].is_none() {
            old_slots
                .entry(prev_task(old.ids, raw))
                .or_default()
                .push(raw);
        }
    }
    let mut new_slots: BTreeMap<Option<TaskId>, Vec<usize>> = BTreeMap::new();
    for raw in 0..new.file.lines.len() {
        if new.ids[raw].is_none() {
            new_slots
                .entry(prev_task(new.ids, raw))
                .or_default()
                .push(raw);
        }
    }
    let anchors: BTreeSet<Option<TaskId>> =
        old_slots.keys().chain(new_slots.keys()).copied().collect();
    let empty = Vec::new();
    for anchor in anchors {
        let olds = old_slots.get(&anchor).unwrap_or(&empty);
        let news = new_slots.get(&anchor).unwrap_or(&empty);
        let keep = olds.len().min(news.len());
        deletes.extend(&olds[keep..]);
        inserts.extend(&news[keep..]);
    }
}

/// Indices into `seq` forming one longest strictly-increasing subsequence of its values.
/// Patience sorting: O(n log n), no recursion.
fn lis_indices(seq: &[usize]) -> Vec<usize> {
    let mut tails: Vec<usize> = Vec::new();
    let mut prev: Vec<Option<usize>> = vec![None; seq.len()];
    for (i, &v) in seq.iter().enumerate() {
        let pos = tails.partition_point(|&ti| seq[ti] < v);
        if pos > 0 {
            prev[i] = Some(tails[pos - 1]);
        }
        if pos == tails.len() {
            tails.push(i);
        } else {
            tails[pos] = i;
        }
    }
    let mut lis = Vec::with_capacity(tails.len());
    let mut cur = tails.last().copied();
    while let Some(i) = cur {
        lis.push(i);
        cur = prev[i];
    }
    lis.reverse();
    lis
}
