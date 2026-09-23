//! Does a plan do what the command did? `plan_mutations` reads a diff as intent-level mutations,
//! and each guess about how the daemon will apply them (a blank paired with a delete, an append at
//! the end) is a place the file can quietly end up different from the one the command printed
//! item numbers for. So a plan is only sent if replaying it on the old lines, the way the daemon
//! applies mutations, gives the new lines exactly; any other diff goes out as a whole-document
//! `Replace` (`daemon_mode.rs`), which is right by construction.

use txtodo_core::File;
use txtodo_proto::v1::{self as pb, mutation::Kind};

type Lines = Vec<Vec<u8>>;

/// Where the daemon puts an appended or moved task: right after the last non-blank line, before
/// any trailing blanks (`add_ops`/`move_to_end_ops` anchor on `DocState::task_before(len)`), or
/// first when there is none. Direct mode appends after the blanks, so this is where they differ.
fn tail(lines: &[Vec<u8>]) -> usize {
    lines
        .iter()
        .rposition(|l| !l.is_empty())
        .map_or(0, |i| i + 1)
}

/// The 0-based index a mutation names, if that line exists and is a task: the daemon refuses to
/// address a blank one.
fn task_index(lines: &[Vec<u8>], task: Option<&pb::TaskRef>) -> Option<usize> {
    let at = usize::try_from(task?.line_number).ok()?.checked_sub(1)?;
    lines.get(at).filter(|l| !l.is_empty()).map(|_| at)
}

/// `muts` applied to `old` the way the daemon applies them; `None` for a mutation a plan never
/// holds or one the daemon would refuse.
fn replay(old: &File, muts: &[pb::Mutation]) -> Option<Lines> {
    let mut lines: Lines = old.lines.iter().map(|l| l.bytes().to_vec()).collect();
    for kind in muts.iter().filter_map(|m| m.kind.as_ref()) {
        match kind {
            Kind::Edit(e) => {
                let at = task_index(&lines, e.task.as_ref())?;
                lines[at] = e.new_line.clone().into_bytes();
            }
            Kind::Complete(c) => {
                let at = task_index(&lines, c.task.as_ref())?;
                let done = crate::complete_plan::completed(&lines[at], &c.today)?;
                // The daemon's `Complete` also moves a line it changed to the end of its file,
                // unless it is the last task already (`mutation.rs`, task complete-to-bottom).
                let changed = done != lines[at];
                lines[at] = done;
                if changed && at + 1 != tail(&lines) {
                    let moved = lines.remove(at);
                    lines.insert(tail(&lines), moved);
                }
            }
            Kind::Delete(d) => {
                let at = task_index(&lines, d.task.as_ref())?;
                if d.leave_blank {
                    lines[at].clear();
                } else {
                    lines.remove(at);
                }
            }
            Kind::MoveToEnd(m) => {
                let moved = lines.remove(task_index(&lines, m.task.as_ref())?);
                lines.insert(tail(&lines), moved);
            }
            Kind::Add(a) => lines.insert(tail(&lines), a.line.clone().into_bytes()),
            _ => return None,
        }
    }
    Some(lines)
}

/// True when applying `muts` to `old` yields `new`, line for line.
pub fn reproduces(old: &File, new: &File, muts: &[pb::Mutation]) -> bool {
    let want = new.lines.iter().map(|l| l.bytes());
    replay(old, muts).is_some_and(|got| got.iter().map(Vec::as_slice).eq(want))
}

#[cfg(test)]
mod tests {
    use crate::daemon_mode::plan_mutations;
    use txtodo_core::parse_file;

    fn planned(old: &str, new: &str) -> bool {
        plan_mutations(&parse_file(old.as_bytes()), &parse_file(new.as_bytes())).is_some()
    }

    #[test]
    fn a_del_that_leaves_a_blank_and_a_plain_append_are_still_mutations() {
        assert!(
            planned("a\nb\nc\n", "a\n\nc\n"),
            "del 2 leaves a blank in place"
        );
        assert!(
            planned("a\nb\nc\n", "a\nb\nc\nd\n"),
            "an append with no trailing blank"
        );
        assert!(planned("a\nb\nc\n", "a\nc\n"), "a plain delete");
    }

    #[test]
    fn an_append_after_a_trailing_blank_is_a_replace_because_the_daemon_puts_it_before_the_blank() {
        // Direct mode appends after the blank (line 3); the daemon would anchor the new task on the
        // last task and put it on line 2, so the item number `add` printed would be wrong.
        assert!(!planned("a\n\n", "a\n\nb\n"));
    }

    #[test]
    fn several_deletes_around_a_blank_are_a_replace_because_the_blank_pairing_is_by_index_only() {
        // Deletes of `a` and `b`, and a blank appended at index 1: pairing that blank with `b`
        // (also index 1) would leave it in `b`'s old place, ahead of `c`, not after it.
        assert!(!planned("a\nb\nc\n", "c\n\n"));
    }

    #[test]
    fn a_rewritten_last_line_is_a_replace_not_a_delete_plus_an_add() {
        // Sidecar text: no ids, so a changed last line reads as "delete a line, append a line".
        assert!(!planned("a\nb\n", "a\n(A) b\n"));
        assert!(!planned("a\n", "(A) a\n"));
    }
}
