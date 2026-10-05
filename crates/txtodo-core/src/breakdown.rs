//! A line field by field (task `tui-desktop-fixes-sync`), like sqlite's `.mode line`: one
//! `(name, value)` row per part of the todo.txt syntax, so a long line can be read without parsing
//! it by eye. Here so the TUI and (through `txtodo-ffi`) desktop show the same rows (ADR 0031).
//! Ref: <https://sqlite.org/cli.html#changing_output_formats>

use alloc::borrow::ToOwned;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::{LineKind, Mode, parse_line};

/// The rows for `line`: `done`, `priority`, `created`, `completed` and `text` always (empty when
/// absent), then `projects` and `contexts` when there are any, then each `key:value` tag in order.
/// A blank line has no rows.
pub fn fields(line: &str) -> Vec<(String, String)> {
    // Lenient parsing reads whitespace alone as a task with leading-space quirks; it is blank.
    if line.trim().is_empty() {
        return Vec::new();
    }
    let Ok(LineKind::Task(task)) = parse_line(line, Mode::Lenient).map(|l| l.kind) else {
        return Vec::new();
    };
    let date = |d: Option<crate::Date>| d.map_or_else(String::new, |d| d.to_string());
    let joined = |sigil: &str, words: &mut dyn Iterator<Item = &str>| {
        words
            .map(|w| [sigil, w].concat())
            .collect::<Vec<_>>()
            .join(" ")
    };
    let mut rows: Vec<(String, String)> = Vec::new();
    let mut row = |name: &str, value: String| rows.push((name.to_owned(), value));
    row("done", if task.completed { "yes" } else { "no" }.to_owned());
    row(
        "priority",
        task.priority.map_or_else(String::new, |p| p.to_string()),
    );
    row("created", date(task.creation_date));
    row("completed", date(task.completion_date));
    row("text", task.plain_words().collect::<Vec<_>>().join(" "));
    let projects = joined("+", &mut task.projects());
    if !projects.is_empty() {
        row("projects", projects);
    }
    let contexts = joined("@", &mut task.contexts());
    if !contexts.is_empty() {
        row("contexts", contexts);
    }
    for (key, value) in task.tags() {
        row(key, value.to_owned());
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    fn named(rows: &[(String, String)], name: &str) -> Option<String> {
        rows.iter().find(|(n, _)| n == name).map(|(_, v)| v.clone())
    }

    #[test]
    fn a_line_comes_apart_into_its_fields() {
        let rows = fields("(A) 2026-10-05 call mum +home @phone due:2026-10-09 ref:mum");
        let names: Vec<&str> = rows.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(
            names,
            [
                "done",
                "priority",
                "created",
                "completed",
                "text",
                "projects",
                "contexts",
                "due",
                "ref"
            ]
        );
        assert_eq!(named(&rows, "priority").as_deref(), Some("(A)"));
        assert_eq!(named(&rows, "text").as_deref(), Some("call mum"));
        assert_eq!(named(&rows, "contexts").as_deref(), Some("@phone"));
        assert_eq!(named(&rows, "completed").as_deref(), Some(""));
    }

    #[test]
    fn a_done_line_says_so_and_a_blank_line_has_no_rows() {
        let rows = fields("x 2026-10-05 2026-10-01 call mum pri:B");
        assert_eq!(named(&rows, "done").as_deref(), Some("yes"));
        assert_eq!(named(&rows, "completed").as_deref(), Some("2026-10-05"));
        assert_eq!(named(&rows, "pri").as_deref(), Some("B"));
        assert!(fields("   ").is_empty());
    }
}
