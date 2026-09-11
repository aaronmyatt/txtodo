//! `--json`: one JSON object per listed line (plan M2): `line`, `raw`, the parsed fields, and `spans`
//! from `tokenize`. Hand-built per RFC 8259 — the shapes are flat and fixed, no serde needed.
//! https://www.rfc-editor.org/rfc/rfc8259#section-7

use crate::commands::list::Item;
use txtodo_core::{LineKind, Mode, Task, parse_line, tokenize};

/// A JSON string literal: quotes, backslashes and control characters escaped.
pub fn str(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    debug_assert!(out.len() >= s.len() + 2, "quotes added");
    debug_assert!(!out[1..out.len() - 1].contains('\n'), "newlines escaped");
    out
}

/// A JSON array of strings.
pub fn strs<'a>(items: impl Iterator<Item = &'a str>) -> String {
    format!("[{}]", items.map(str).collect::<Vec<_>>().join(","))
}

fn opt<T: ToString>(v: Option<T>) -> String {
    v.map_or_else(|| "null".to_string(), |v| str(&v.to_string()))
}

/// The task fields as `"key":value` pairs (design §3 views: projects, contexts, tags, id).
fn task_fields(t: &Task<'_>) -> String {
    let tags: Vec<String> = t
        .tags()
        .map(|(k, v)| format!("{}:{}", str(k), str(v)))
        .collect();
    format!(
        r#""completed":{},"completion_date":{},"creation_date":{},"priority":{},"description":{},"projects":{},"contexts":{},"tags":{{{}}},"id":{}"#,
        t.completed,
        opt(t.completion_date),
        opt(t.creation_date),
        opt(t.priority.map(|p| p.as_char())),
        str(t.description),
        strs(t.projects()),
        strs(t.contexts()),
        tags.join(","),
        opt(t.id()),
    )
}

/// One listed line as a JSON object. A blank line has `"task":false` and no task fields.
pub fn item(it: &Item) -> String {
    let spans: Vec<String> = tokenize(&it.raw)
        .iter()
        .map(|s| {
            format!(
                r#"{{"kind":"{:?}","start":{},"end":{}}}"#,
                s.kind, s.start, s.end
            )
        })
        .collect();
    let (task, quirks) = match parse_line(&it.raw, Mode::Lenient) {
        Ok(line) => {
            let quirks = strs(line.quirks.names());
            match line.kind {
                LineKind::Task(t) => (format!(r#""task":true,{}"#, task_fields(&t)), quirks),
                LineKind::Blank => (r#""task":false"#.to_string(), quirks),
            }
        }
        Err(_) => (r#""task":false"#.to_string(), "[]".to_string()),
    };
    let out = format!(
        r#"{{"line":{},"raw":{},{task},"quirks":{quirks},"spans":[{}]}}"#,
        it.number,
        str(&it.raw),
        spans.join(","),
    );
    debug_assert!(out.starts_with('{') && out.ends_with('}'), "one object");
    debug_assert!(!out.contains('\n'), "one line");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strings_escape_quotes_backslashes_and_controls() {
        assert_eq!(str("a\"b\\c\nd"), r#""a\"b\\c\nd""#);
        assert_eq!(str("\u{1}"), r#""\u0001""#);
    }

    #[test]
    fn a_task_serialises_every_field_and_a_blank_has_no_task() {
        let it = Item {
            number: 3,
            raw: "(A) 2026-09-11 Call +home @phone due:2026-09-12".into(),
        };
        let j = item(&it);
        let head = concat!(
            r#"{"line":3,"raw":"(A) 2026-09-11 Call +home @phone due:2026-09-12","task":true,"#,
            r#""completed":false,"completion_date":null,"creation_date":"2026-09-11","priority":"A","#,
            r#""description":"Call +home @phone due:2026-09-12","projects":["home"],"contexts":["phone"],"#,
            r#""tags":{"due":"2026-09-12"},"id":null,"quirks":[],"spans":[{"kind":"Priority","start":0,"end":3},"#
        );
        assert!(j.starts_with(head), "{j}");
        assert!(
            j.ends_with(r#"{"kind":"TagValue","start":37,"end":47}]}"#),
            "{j}"
        );
        let blank = item(&Item {
            number: 1,
            raw: String::new(),
        });
        assert_eq!(
            blank,
            r#"{"line":1,"raw":"","task":false,"quirks":[],"spans":[]}"#
        );
    }
}
