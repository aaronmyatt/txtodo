//! What the daemon does to a line an app writes, before it lands (task
//! `tui-desktop-fixes-sync`): today's creation date on a new line, and today's completion date on
//! a line typed done (`x ` and no date). Here, not in each client, so
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
    fn a_dated_or_done_line_is_left_alone() {
        assert_eq!(stamp_created("2026-01-01 old", today()), "2026-01-01 old");
        assert_eq!(
            stamp_created("x 2026-01-01 done", today()),
            "x 2026-01-01 done"
        );
    }
}
