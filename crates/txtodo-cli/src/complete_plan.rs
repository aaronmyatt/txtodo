//! `do` in daemon mode as the daemon's own `Complete` mutation (root todo "daemon-mode do sends
//! Edit plus MoveToEnd, so the op log never says complete"). `do` completes each named line and
//! moves it to the end of the file (`commands/edit.rs::move_to_end`); the daemon's `Complete`
//! does both in one mutation (task complete-to-bottom), so when the scratch-copy diff has exactly
//! that shape the plan is one `Complete` per line, and `txtodo log`, the activity pane and
//! `todo_history` say `complete`. Any other shape (`do -A` leaves the line in place; a completion
//! typed by hand) goes on to `archive_plan` and the plain edit path as before.
//!
//! Lines match by `id:` (the daemon's tagged projection) or, under the sidecar default, by their
//! bytes — the daemon then addresses a line by number alone, as the plain edit path already does.
//! A blank line or a repeated key means this planner stands aside.

use std::collections::HashSet;
use txtodo_core::{Date, Edit, File, LineEnding, LineKind, OwnedLine, Ulid, apply};
use txtodo_proto::v1::{self as pb, mutation};

/// The completion date when `new` is `old` completed by `Edit::complete` and nothing else — the
/// `x YYYY-MM-DD` prefix (and a priority moved to `pri:`), byte for byte.
pub fn completion_of(old: &OwnedLine, new: &OwnedLine) -> Option<Date> {
    let (Some(before), Some(after)) = (old.parse(), new.parse()) else {
        return None;
    };
    let (LineKind::Task(before), LineKind::Task(after)) = (before.kind, after.kind) else {
        return None;
    };
    if before.completed || !after.completed {
        return None;
    }
    let today = after.completion_date?;
    (apply(old, &Edit::new().complete(today)).bytes() == new.bytes()).then_some(today)
}

/// `line` completed on `today` (`YYYY-MM-DD`), as the daemon's `Complete` rewrites it; `None` for
/// a date the daemon would refuse.
pub fn completed(line: &[u8], today: &str) -> Option<Vec<u8>> {
    let today = Date::parse(today)?;
    let line = OwnedLine::from_bytes(line.to_vec(), LineEnding::default());
    Some(apply(&line, &Edit::new().complete(today)).bytes().to_vec())
}

/// What identifies a line across the diff: its `id:` when it has one, else its bytes.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
enum Key {
    Id(Ulid),
    Content(Vec<u8>),
}

fn key_of(line: &OwnedLine) -> Option<Key> {
    match line.parse()?.kind {
        LineKind::Task(t) => Some(
            t.id()
                .map_or_else(|| Key::Content(line.bytes().to_vec()), Key::Id),
        ),
        LineKind::Blank => None,
    }
}

/// The keys in line order; `None` when a line is blank or a key repeats.
fn keys_of(file: &File) -> Option<Vec<Key>> {
    let keys: Vec<Key> = file.lines.iter().map(key_of).collect::<Option<_>>()?;
    // https://doc.rust-lang.org/std/collections/struct.HashSet.html
    let unique: HashSet<&Key> = keys.iter().collect();
    (unique.len() == keys.len()).then_some(keys)
}

/// Where `old`'s line `i` is in `new`: the line with the same key, or — for a content-keyed line
/// `do` completed, whose bytes changed — the one line that is `old`'s completed on some day.
fn counterpart(old: &File, i: usize, new: &File, new_keys: &[Key]) -> Option<usize> {
    let key = key_of(&old.lines[i])?;
    if let Some(at) = new_keys.iter().position(|k| *k == key) {
        return Some(at);
    }
    let mut hits =
        (0..new.lines.len()).filter(|&at| completion_of(&old.lines[i], &new.lines[at]).is_some());
    let hit = hits.next()?;
    hits.next().is_none().then_some(hit)
}

/// One `Complete` per line, in old order, when `new` is `old` with some lines completed and those
/// lines moved to the end in their old order (`do`'s own shape) — and every other line unchanged.
/// `None` for any other diff, including no completion at all.
pub fn plan(old: &File, new: &File) -> Option<Vec<pb::Mutation>> {
    let new_keys = keys_of(new)?;
    if keys_of(old)?.len() != new_keys.len() {
        return None;
    }
    // Each old line's place in `new`: the open ones first, then the completed ones, in old order.
    let (mut open, mut done, mut out) = (Vec::new(), Vec::new(), Vec::new());
    for i in 0..old.lines.len() {
        let at = counterpart(old, i, new, &new_keys)?;
        let (before, after) = (&old.lines[i], &new.lines[at]);
        if after.bytes() == before.bytes() {
            open.push(at);
            continue;
        }
        let today = completion_of(before, after)?;
        // Each earlier completion lifted one line from above this one to the end, so the k-th
        // sits k lines higher than `old` numbered it; a tagged id guards the position.
        let mut task = crate::daemon_mode::task_ref(old, i);
        task.line_number = u32::try_from(i - done.len() + 1).ok()?;
        out.push(pb::Mutation {
            kind: Some(mutation::Kind::Complete(pb::Complete {
                task: Some(task),
                today: today.to_string(),
            })),
        });
        done.push(at);
    }
    if done.is_empty() {
        return None;
    }
    open.append(&mut done);
    (open == (0..new.lines.len()).collect::<Vec<_>>()).then_some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use txtodo_core::parse_file;

    fn line(text: &str) -> OwnedLine {
        parse_file(text.as_bytes()).lines.remove(0)
    }

    fn id(n: usize) -> String {
        format!("id:01ARZ3NDEKTSV4RRFFQ69G5FA{n}")
    }

    fn file(lines: &[String]) -> File {
        parse_file((lines.join("\n") + "\n").as_bytes())
    }

    fn line_numbers(muts: &[pb::Mutation]) -> Vec<u32> {
        muts.iter()
            .map(|m| match &m.kind {
                Some(mutation::Kind::Complete(c)) => c.task.as_ref().map_or(0, |t| t.line_number),
                other => panic!("not a Complete: {other:?}"),
            })
            .collect()
    }

    #[test]
    fn a_completion_is_recognised_with_and_without_a_priority() {
        let plain = completion_of(
            &line("call mum @phone"),
            &line("x 2026-09-23 call mum @phone"),
        );
        assert_eq!(plain.map(|d| d.to_string()).as_deref(), Some("2026-09-23"));
        let pri = completion_of(
            &line("(A) 2026-09-01 call mum"),
            &line("x 2026-09-23 2026-09-01 call mum pri:A"),
        );
        assert_eq!(pri.map(|d| d.to_string()).as_deref(), Some("2026-09-23"));
        assert_eq!(
            completion_of(&line("call mum"), &line("call mum +home")),
            None
        );
        assert_eq!(
            completion_of(&line("call mum"), &line("x 2026-09-23 call mum +home")),
            None,
            "completed and edited at once is not a bare completion"
        );
        assert_eq!(
            completion_of(&line("x 2026-09-23 call mum"), &line("call mum")),
            None,
            "reopening is not a completion"
        );
    }

    #[test]
    fn do_of_several_lines_plans_one_complete_each_with_shifted_line_numbers() {
        let t: Vec<String> = (0..4).map(|n| format!("t{n} {}", id(n))).collect();
        // `do 1 3` on t0..t3: t0 and t2 completed and moved to the end, in that order.
        let new = [
            t[1].clone(),
            t[3].clone(),
            format!("x 2026-09-23 {}", t[0]),
            format!("x 2026-09-23 {}", t[2]),
        ];
        let muts = plan(&file(&t), &file(&new)).unwrap_or_else(|| panic!("do shape"));
        // t2 was line 3; once t0 left, it is line 2.
        assert_eq!(line_numbers(&muts), [1, 2]);
        // Completing the last line moves nothing, and is still one Complete.
        let last = [t[0].clone(), format!("x 2026-09-23 {}", t[1])];
        let muts = plan(&file(&t[..2]), &file(&last)).unwrap_or_else(|| panic!("last line"));
        assert_eq!(line_numbers(&muts), [2]);
    }

    #[test]
    fn sidecar_text_without_ids_matches_by_bytes() {
        let old = parse_file(b"a\nb\nc\n");
        let muts =
            plan(&old, &parse_file(b"b\nc\nx 2026-09-23 a\n")).unwrap_or_else(|| panic!("do 1"));
        assert_eq!(line_numbers(&muts), [1]);
        let muts = plan(&old, &parse_file(b"c\nx 2026-09-23 a\nx 2026-09-23 b\n"))
            .unwrap_or_else(|| panic!("do 1 2"));
        assert_eq!(line_numbers(&muts), [1, 1]);
        assert!(
            plan(&parse_file(b"a\na\n"), &parse_file(b"a\nx 2026-09-23 a\n")).is_none(),
            "two lines with the same bytes: which one was done is unknowable"
        );
    }

    #[test]
    fn any_other_shape_stands_aside() {
        let t: Vec<String> = (0..3).map(|n| format!("t{n} {}", id(n))).collect();
        let done0 = format!("x 2026-09-23 {}", t[0]);
        assert!(
            plan(
                &file(&t),
                &file(&[done0.clone(), t[1].clone(), t[2].clone()])
            )
            .is_none(),
            "do -A: completed in place"
        );
        assert!(
            plan(
                &file(&t),
                &file(&[t[2].clone(), t[1].clone(), done0.clone()])
            )
            .is_none(),
            "other lines reordered too"
        );
        assert!(
            plan(&file(&t), &file(&[t[1].clone(), t[2].clone()])).is_none(),
            "a line went missing"
        );
        assert!(plan(&file(&t), &file(&t)).is_none(), "nothing completed");
        assert!(
            plan(
                &parse_file(b"a\n\nb\n"),
                &parse_file(b"\nb\nx 2026-09-23 a\n")
            )
            .is_none(),
            "a blank line"
        );
    }
}
