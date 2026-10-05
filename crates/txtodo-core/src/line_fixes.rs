//! What the daemon does to a line an app writes, before it lands (task
//! `tui-desktop-fixes-sync`): today's creation date on a new line, today's completion date on a
//! line typed done (`x ` and no date), and the case slips fixed (`X ` to `x `, `(a) ` to `(A) `). Here, not in each client, so
//! the CLI, the TUI, the desktop app and MCP all get the same line for the same input (ADR 0031:
//! shared logic lives in core). Pure text in, text out; the caller supplies today's local date
//! (ADR 0011).
//! Ref: <https://github.com/todotxt/todo.txt#rule-2-a-tasks-creation-date-may-optionally-appear-directly-after-priority-and-a-space>

use alloc::format;
use alloc::string::{String, ToString};

use crate::{Date, Edit, LineEnding, LineKind, Mode, OwnedLine, apply, parse_line};

/// Inserts `today ` after an optional `(P) ` prefix (todo.sh `-t`), unless the line already carries
/// a creation date or is completed: the spec has one creation date, and todo.sh would stamp twice.
pub fn stamp_created(line: &str, today: Date) -> String {
    let dated = match parse_line(line, Mode::Lenient).map(|l| l.kind) {
        Ok(LineKind::Task(t)) => t.completed || t.creation_date.is_some(),
        Ok(LineKind::Blank) | Err(_) => false,
    };
    if dated {
        return line.to_string();
    }
    let at = match line.as_bytes() {
        [b'(', p, b')', b' ', ..] if p.is_ascii_uppercase() => 4,
        _ => 0,
    };
    let out = format!("{}{today} {}", &line[..at], &line[at..]);
    debug_assert!(out.len() == line.len() + 11, "one date and one space added");
    out
}

/// A line typed done, `x ` with no completion date after it, becomes done today the way
/// `Complete` makes it (a priority turns into `pri:`). Any other line is returned as it is.
pub fn date_done(line: &str, today: Date) -> String {
    let Some(rest) = line.strip_prefix("x ") else {
        return line.to_string();
    };
    // Lenient parsing counts `x call mum` as done with no date; only that case is stamped.
    let dated = matches!(
        parse_line(line, Mode::Lenient).map(|l| l.kind),
        Ok(LineKind::Task(t)) if t.completion_date.is_some()
    );
    if dated {
        return line.to_string();
    }
    let open = OwnedLine::from_bytes(rest.as_bytes().to_vec(), LineEnding::default());
    let done = apply(&open, &Edit::new().complete(today));
    let out = done.raw().unwrap_or(line).to_string();
    debug_assert!(out.starts_with("x "), "still a done line");
    out
}

/// Fixes the two case slips the spec has no room for, only where they are unambiguous: a leading
/// `X ` becomes the done marker `x `, and a lower-case priority `(a) ` becomes `(A) ` at the start
/// of the line or after the done marker and its dates. Anywhere else `X` and `(a)` are words.
pub fn fix_case(line: &str) -> String {
    let mut out = line.to_string();
    if out.starts_with("X ") {
        out.replace_range(0..1, "x");
    }
    let mut at = 0;
    if out.starts_with("x ") {
        at = 2;
        // Up to two dates after the marker: completion, then creation.
        for _ in 0..2 {
            if out.get(at..at + 10).and_then(Date::parse).is_some()
                && out[at + 10..].starts_with(' ')
            {
                at += 11;
            }
        }
    }
    if let [b'(', p, b')', b' ', ..] = out.as_bytes()[at..]
        && p.is_ascii_lowercase()
    {
        let upper = char::from(p.to_ascii_uppercase());
        out.replace_range(at + 1..at + 2, upper.encode_utf8(&mut [0; 4]));
    }
    debug_assert!(out.len() == line.len(), "case fixes keep the length");
    out
}

/// Whether `line` is done: lenient parsing, so `x call mum` (no date yet) counts.
pub fn is_completed(line: &str) -> bool {
    matches!(
        parse_line(line, Mode::Lenient).map(|l| l.kind),
        Ok(LineKind::Task(t)) if t.completed
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn today() -> Date {
        Date::new(2026, 9, 11).unwrap_or_else(|| unreachable!("a real date"))
    }

    #[test]
    fn a_new_line_gets_todays_date_after_its_priority() {
        assert_eq!(stamp_created("call mum", today()), "2026-09-11 call mum");
        assert_eq!(
            stamp_created("(B) call mum", today()),
            "(B) 2026-09-11 call mum"
        );
        assert_eq!(stamp_created("", today()), "2026-09-11 ");
    }

    #[test]
    fn a_line_typed_done_gets_todays_completion_date() {
        assert_eq!(date_done("x call mum", today()), "x 2026-09-11 call mum");
        assert_eq!(
            date_done("x (A) call mum", today()),
            "x 2026-09-11 call mum pri:A",
            "as Complete does it"
        );
        assert_eq!(
            date_done("x 2026-09-01 call mum", today()),
            "x 2026-09-01 call mum",
            "already dated"
        );
        assert_eq!(date_done("xylophone", today()), "xylophone");
        assert!(is_completed("x 2026-09-01 a") && is_completed("x a") && !is_completed("a"));
    }

    #[test]
    fn case_slips_are_fixed_only_where_unambiguous() {
        assert_eq!(fix_case("X call mum"), "x call mum");
        assert_eq!(fix_case("(a) call mum"), "(A) call mum");
        assert_eq!(fix_case("x (b) call mum"), "x (B) call mum");
        assert_eq!(
            fix_case("x 2026-09-11 2026-09-01 (c) call"),
            "x 2026-09-11 2026-09-01 (C) call"
        );
        assert_eq!(fix_case("X-ray the pipes"), "X-ray the pipes");
        assert_eq!(fix_case("call (a) mum X ok"), "call (a) mum X ok");
        assert_eq!(fix_case("(ab) not a priority"), "(ab) not a priority");
    }

    #[test]
    fn a_dated_or_done_line_is_left_alone() {
        assert_eq!(stamp_created("2026-01-01 old", today()), "2026-01-01 old");
        assert_eq!(
            stamp_created("x 2026-01-01 done", today()),
            "x 2026-01-01 done"
        );
    }
}
