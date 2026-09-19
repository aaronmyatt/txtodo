//! `archive`'s reorder (done lines pushed to the bottom, in their old order) as guarded
//! `MoveToEnd` mutations, so `do` + auto-archive no longer needs `daemon_mode`'s whole-file
//! fallback, which overwrites the file from a snapshot and can silently drop another agent's
//! concurrent `Apply`. Recognised by shape, not by `diff_lines`' `Move` steps: Myers may pick any
//! longest common subsequence, so which lines it calls "moved" is arbitrary.
//!
//! Every line must carry an `id:` (the daemon's own projection always does) and no line may be
//! blank: archive also drops blanks, and no mutation removes one, so a file with blanks still
//! falls back. Each mutation names its line and id, so a stale snapshot is refused as a whole
//! (one `Apply` is atomic) instead of moving the wrong task.

use crate::daemon_mode::{line_text, task_ref};
use std::collections::{HashMap, HashSet};
use txtodo_core::{File, LineKind, OwnedLine, Ulid};
use txtodo_proto::v1::{self as pb, mutation};

/// The daemon refuses an `Apply` of more mutations than this (`MAX_MUTATIONS_PER_APPLY` in
/// `txtodo-daemon`'s `mutation.rs`; this crate does not depend on the daemon). A bigger plan falls
/// back rather than being refused.
const MAX_BATCH: usize = 10_000;

fn id_of(line: &OwnedLine) -> Option<Ulid> {
    match line.parse()?.kind {
        LineKind::Task(t) => t.id(),
        LineKind::Blank => None,
    }
}

/// The ids in line order; `None` when a line is blank, has no valid `id:`, or an id repeats.
fn ids_of(file: &File) -> Option<Vec<Ulid>> {
    let ids: Vec<Ulid> = file.lines.iter().map(id_of).collect::<Option<_>>()?;
    // https://doc.rust-lang.org/std/collections/struct.HashSet.html
    let unique: HashSet<&Ulid> = ids.iter().collect();
    (unique.len() == ids.len()).then_some(ids)
}

/// `archive`'s own test for a done line (`commands/archive.rs::split`).
fn is_done(line: &OwnedLine) -> bool {
    line.bytes().starts_with(b"x ")
}

fn wrap(kind: mutation::Kind) -> pb::Mutation {
    pb::Mutation { kind: Some(kind) }
}

/// An `Edit` for every line whose bytes differ between the files (`do` completes a line), sent
/// first: an edit never shifts a line, so every one still names its line as `old` numbered it.
fn edits(
    old: &File,
    new: &File,
    old_ids: &[Ulid],
    at: &HashMap<Ulid, usize>,
) -> Option<Vec<pb::Mutation>> {
    let mut out = Vec::new();
    for (i, id) in old_ids.iter().enumerate() {
        let line = new.lines.get(*at.get(id)?)?;
        if line.bytes() != old.lines[i].bytes() {
            out.push(wrap(mutation::Kind::Edit(pb::Edit {
                task: Some(task_ref(old, i)),
                new_line: line_text(line)?,
            })));
        }
    }
    Some(out)
}

/// One `MoveToEnd` per done line, in old order, so they land at the bottom in that order. Each
/// earlier move lifted one line from above this one, so the k-th sits k lines higher than `old`
/// numbered it; the id is what guards the position.
fn moves(old: &File, old_ids: &[Ulid], done: &HashSet<Ulid>) -> Option<Vec<pb::Mutation>> {
    let done_at = old_ids
        .iter()
        .enumerate()
        .filter(|(_, id)| done.contains(*id));
    let mut out = Vec::new();
    for (k, (i, _)) in done_at.enumerate() {
        let mut task = task_ref(old, i);
        task.line_number = u32::try_from(i - k + 1).ok()?;
        out.push(wrap(mutation::Kind::MoveToEnd(pb::MoveToEnd {
            task: Some(task),
        })));
    }
    Some(out)
}

/// The mutations that turn `old` into `new`, when `new` is exactly `old` archived: not-done lines
/// keep their order, done lines follow in theirs, and done-ness may have changed along the way.
/// `None` for any other shape, and for a file that is already in that order (no move needed, so
/// `daemon_mode`'s plain edit path handles it).
pub fn plan(old: &File, new: &File) -> Option<Vec<pb::Mutation>> {
    let (old_ids, new_ids) = (ids_of(old)?, ids_of(new)?);
    let done: HashSet<Ulid> = new
        .lines
        .iter()
        .zip(&new_ids)
        .filter(|(line, _)| is_done(line))
        .map(|(_, id)| *id)
        .collect();
    // https://doc.rust-lang.org/std/iter/trait.Iterator.html#method.partition
    let (open, closed): (Vec<Ulid>, Vec<Ulid>) = old_ids.iter().partition(|id| !done.contains(*id));
    let archived: Vec<Ulid> = open.into_iter().chain(closed).collect();
    if archived != new_ids || old_ids == new_ids {
        return None;
    }
    let at: HashMap<Ulid, usize> = new_ids.iter().copied().zip(0..).collect();
    let mut out = edits(old, new, &old_ids, &at)?;
    out.extend(moves(old, &old_ids, &done)?);
    (out.len() <= MAX_BATCH).then_some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use txtodo_core::parse_file;

    fn id(n: usize) -> String {
        format!("01ARZ3NDEKTSV4RRFFQ69G5FA{n}")
    }

    /// Task `n`'s line, done or not.
    fn line(n: usize, done: bool) -> String {
        let mark = if done { "x 2026-09-19 " } else { "" };
        format!("{mark}t{n} id:{}", id(n))
    }

    fn file(lines: &[String]) -> File {
        parse_file((lines.join("\n") + "\n").as_bytes())
    }

    /// Replays `plan` on `old` the way the daemon would: every mutation must find the id it names
    /// at the line it names, then an edit replaces the line and a move sends it to the bottom.
    fn replay(old: &[String], muts: &[pb::Mutation]) -> Vec<String> {
        let mut lines = old.to_vec();
        for m in muts {
            let (task, new_line) = match m.kind.clone() {
                Some(mutation::Kind::Edit(e)) => (e.task.unwrap(), Some(e.new_line)),
                Some(mutation::Kind::MoveToEnd(e)) => (e.task.unwrap(), None),
                other => panic!("unexpected {other:?}"),
            };
            let at = task.line_number as usize - 1;
            assert!(
                lines[at].ends_with(&task.task_id),
                "guard: {lines:?} vs {task:?}"
            );
            match new_line {
                Some(text) => lines[at] = text,
                None => {
                    let moved = lines.remove(at);
                    lines.push(moved);
                }
            }
        }
        lines
    }

    #[test]
    fn do_in_the_middle_edits_then_moves_every_done_line_in_old_order() {
        // t0 was just done; t2 and t3 were done already. Archive puts t0 ahead of them (old
        // order), so MoveToEnd, which can only append, has to move all three, not just t0.
        let old = [line(0, false), line(1, false), line(2, true), line(3, true)];
        let new = [line(1, false), line(0, true), line(2, true), line(3, true)];
        let (o, n) = (file(&old), file(&new));
        let muts = plan(&o, &n).unwrap();
        let kinds: Vec<&str> = muts
            .iter()
            .map(|m| match m.kind {
                Some(mutation::Kind::Edit(_)) => "edit",
                _ => "move",
            })
            .collect();
        assert_eq!(kinds, ["edit", "move", "move", "move"]);
        assert_eq!(replay(&old, &muts), new);
    }

    #[test]
    fn the_last_open_line_done_needs_no_move_and_blanks_or_missing_ids_fall_back() {
        let (a, b) = (line(0, false), line(1, false));
        let done_last = [a.clone(), line(1, true)];
        assert!(
            plan(&file(&[a.clone(), b]), &file(&done_last)).is_none(),
            "plain edit path"
        );
        let blank = parse_file(format!("{a}\n\n").as_bytes());
        assert!(
            plan(&blank, &file(&[line(0, true)])).is_none(),
            "blank line: falls back"
        );
        assert!(
            plan(&parse_file(b"a\nb\n"), &parse_file(b"b\na\n")).is_none(),
            "no ids"
        );
    }

    fn bit(mask: u32, i: usize) -> bool {
        (mask >> i) & 1 == 1
    }

    /// Every done-ness pattern before and after, over 5 tasks: whenever `plan` answers, replaying
    /// it reproduces `new` exactly; it declines only when no reorder is needed.
    #[test]
    fn every_archive_shape_replays_to_exactly_the_archived_file() {
        let n = 5;
        for before in 0..(1u32 << n) {
            for after in 0..(1u32 << n) {
                let old: Vec<String> = (0..n).map(|i| line(i, bit(before, i))).collect();
                let mut order: Vec<usize> = (0..n).filter(|&i| !bit(after, i)).collect();
                order.extend((0..n).filter(|&i| bit(after, i)));
                let new: Vec<String> = order.iter().map(|&i| line(i, bit(after, i))).collect();
                match plan(&file(&old), &file(&new)) {
                    Some(muts) => {
                        assert_eq!(replay(&old, &muts), new, "{before:05b} -> {after:05b}")
                    }
                    None => assert!(
                        order.iter().copied().eq(0..n),
                        "declined a reorder: {before:05b} -> {after:05b}"
                    ),
                }
            }
        }
    }
}
