//! Byte-preserving formatter. Only the fields an [`crate::Edit`] touched are rewritten; everything else is
//! spliced through verbatim (design §2.2 rule 2). What it emits is always the strict grammar (rule 3).

use crate::{Date, LineKind, Mode, Priority, Task};
use alloc::string::String;

/// The structured prefix of a task, owned, as the formatter emits it.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Prefix {
    /// `x`.
    pub completed: bool,
    /// Date after `x`.
    pub completion_date: Option<Date>,
    /// Creation date.
    pub creation_date: Option<Date>,
    /// `(A)`–`(Z)`. On a completed line the grammar has no slot for it; see [`emit_prefix`].
    pub priority: Option<Priority>,
}

impl Prefix {
    /// The prefix of a parsed task.
    pub fn of(task: &Task<'_>) -> Prefix {
        Prefix {
            completed: task.completed,
            completion_date: task.completion_date,
            creation_date: task.creation_date,
            priority: task.priority,
        }
    }
}

/// Emits a strict prefix, ending with one space when non-empty and `has_description` is true.
/// A completed prefix never carries `(A)`: callers move it to a `pri:` tag first (see [`crate::Edit::complete`]).
/// A completed line without a completion date (lenient quirk) is emitted as `x` alone: strict cannot say more.
pub fn emit_prefix(p: &Prefix, has_description: bool) -> String {
    let mut out = String::new();
    if p.completed {
        out.push('x');
        push_date(&mut out, p.completion_date);
    } else if let Some(pri) = p.priority {
        out.push('(');
        out.push(pri.as_char());
        out.push(')');
    }
    push_date(&mut out, p.creation_date);
    if !out.is_empty() && has_description {
        out.push(' ');
    }
    debug_assert!(
        !out.starts_with(' '),
        "prefix never starts with a separator"
    );
    debug_assert!(
        has_description || !out.ends_with(' '),
        "no trailing separator without a description"
    );
    out
}

/// Appends ` YYYY-MM-DD` (or `YYYY-MM-DD` on an empty prefix) when `date` is set.
fn push_date(out: &mut String, date: Option<Date>) {
    let Some(d) = date else {
        return;
    };
    if !out.is_empty() {
        out.push(' ');
    }
    use core::fmt::Write;
    // Writing into a String cannot fail; the Result is the trait's shape.
    let _ = write!(out, "{d}");
    debug_assert!(out.len() >= 10, "a date is ten bytes");
}

/// Byte offset where the description starts in `raw` (lenient parse); `raw.len()` when there is none.
pub fn description_start(raw: &str) -> usize {
    match crate::parse_line(raw, Mode::Lenient).map(|l| l.kind) {
        Ok(LineKind::Task(t)) => raw.len() - t.description.len(),
        _ => raw.len(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(s: &str) -> Option<Date> {
        Date::parse(s)
    }

    #[test]
    fn emits_every_strict_shape() {
        let a = Priority::new('A');
        assert_eq!(
            emit_prefix(
                &Prefix {
                    priority: a,
                    creation_date: d("2026-09-11"),
                    ..Prefix::default()
                },
                true
            ),
            "(A) 2026-09-11 "
        );
        assert_eq!(
            emit_prefix(
                &Prefix {
                    priority: a,
                    ..Prefix::default()
                },
                true
            ),
            "(A) "
        );
        assert_eq!(
            emit_prefix(
                &Prefix {
                    priority: a,
                    ..Prefix::default()
                },
                false
            ),
            "(A)"
        );
    }

    #[test]
    fn emits_every_completed_shape() {
        let a = Priority::new('A');
        assert_eq!(
            emit_prefix(
                &Prefix {
                    completed: true,
                    completion_date: d("2026-09-11"),
                    creation_date: d("2026-09-01"),
                    ..Prefix::default()
                },
                true
            ),
            "x 2026-09-11 2026-09-01 "
        );
        assert_eq!(
            emit_prefix(
                &Prefix {
                    completed: true,
                    completion_date: d("2026-09-11"),
                    priority: a,
                    ..Prefix::default()
                },
                false
            ),
            "x 2026-09-11",
            "no priority slot on completed lines"
        );
        assert_eq!(
            emit_prefix(
                &Prefix {
                    completed: true,
                    ..Prefix::default()
                },
                true
            ),
            "x "
        );
        assert_eq!(emit_prefix(&Prefix::default(), true), "");
    }

    #[test]
    fn description_start_is_after_the_prefix() {
        assert_eq!(description_start("(A) 2026-09-11 Call"), 15);
        assert_eq!(
            description_start("x 2026-09-11 (A) t"),
            17,
            "lenient priority is part of the prefix"
        );
        assert_eq!(description_start("plain"), 0);
        assert_eq!(description_start("x 2026-09-11"), 12);
        assert_eq!(description_start(""), 0);
    }
}
