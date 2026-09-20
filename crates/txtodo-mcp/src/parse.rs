//! A minimal todo.txt line parser and editor, used only because this crate's allowed dependencies
//! (`budgets.json.slices.allowedDeps["txtodo-mcp"]`) are `txtodo-proto` and `txtodo-query` —
//! **not** `txtodo-core` (the real parser) — and `txtodo-query` (the real query language, design
//! §8) is still an empty stub, a separate task. See the crate's "As built" notes for the full
//! deviation writeup. This module exists to let `todo_list`/`todo_get`/`todo_search`/`todo_edit`/
//! `todo_uncomplete` return something real today; replace it with `txtodo-query` once that lands.

use crate::backend::TaskRow;

/// Splits `text` into `(1-based line number, raw line)` pairs, blank lines included (design §6.3:
/// "line" is the current line number over every line).
pub fn lines(text: &str) -> Vec<(u32, &str)> {
    text.lines()
        .enumerate()
        .map(|(i, l)| (i as u32 + 1, l))
        .collect()
}

/// `(pri_len, date_len)`: the length of `(X) ` and of a following `YYYY-MM-DD `, each 0 when
/// absent. Mirrors `txtodo-cli`'s `commands/text.rs::prefix_lens` (todo.sh's own regex), which this
/// crate cannot import (that crate builds a binary only, and txtodo-mcp may not depend on it).
pub fn prefix_lens(raw: &str) -> (usize, usize) {
    let pri = match raw.chars().take(4).collect::<Vec<_>>().as_slice() {
        ['(', p, ')', ' '] => 3 + p.len_utf8(),
        _ => 0,
    };
    (pri, date_len(&raw[pri..]))
}

fn date_len(s: &str) -> usize {
    let b = s.as_bytes();
    let year = b.iter().take_while(|c| c.is_ascii_digit()).count();
    if !(2..=4).contains(&year) {
        return 0;
    }
    let rest = &b[year..];
    let shape = rest.len() >= 7
        && rest[0] == b'-'
        && rest[1..3].iter().all(u8::is_ascii_digit)
        && rest[3] == b'-'
        && rest[4..6].iter().all(u8::is_ascii_digit)
        && rest[6] == b' ';
    if shape { year + 7 } else { 0 }
}

/// Parses one line into a [`TaskRow`]. Never panics on malformed input: anything it cannot
/// classify just lands in `kv` or is skipped, the same "lenient" spirit as the real parser.
pub fn parse_row(line_no: u32, raw: &str) -> TaskRow {
    let mut row = TaskRow {
        line: line_no,
        raw: raw.to_owned(),
        ..TaskRow::default()
    };
    let mut rest = raw;
    if let Some(after) = rest.strip_prefix("x ") {
        row.done = true;
        rest = after;
        if date_len(rest) > 0 {
            let n = date_len(rest);
            row.completed = Some(rest[..n - 1].to_owned());
            rest = &rest[n..];
        }
    }
    if let ['(', p, ')', ' '] = rest.chars().take(4).collect::<Vec<_>>().as_slice() {
        row.priority = Some(*p);
        rest = &rest[3 + p.len_utf8()..];
    }
    if date_len(rest) > 0 {
        let n = date_len(rest);
        row.created = Some(rest[..n - 1].to_owned());
        rest = &rest[n..];
    }
    for word in rest.split_whitespace() {
        classify_word(&mut row, word);
    }
    if row.priority.is_none() {
        row.priority = row
            .kv
            .iter()
            .find(|(k, _)| k == "pri")
            .and_then(|(_, v)| v.chars().next());
    }
    row
}

/// One `+project` / `@context` / `key:value` word into `row`. `id`/`due` get their own field;
/// every other tag lands in `kv`, in line order.
fn classify_word(row: &mut TaskRow, word: &str) {
    if let Some(p) = word.strip_prefix('+').filter(|p| !p.is_empty()) {
        row.projects.push(p.to_owned());
    } else if let Some(c) = word.strip_prefix('@').filter(|c| !c.is_empty()) {
        row.contexts.push(c.to_owned());
    } else if let Some((k, v)) = word
        .split_once(':')
        .filter(|(k, v)| !k.is_empty() && !v.is_empty())
    {
        match k {
            // First `id:` word wins (mirrors the daemon's own fastid.rs scan); a duplicate is
            // left as an ordinary tag rather than silently overwriting the real id.
            "id" if row.id.is_none() => row.id = Some(v.to_owned()),
            "due" => row.due = Some(v.to_owned()),
            _ => row.kv.push((k.to_owned(), v.to_owned())),
        }
    }
}

/// Finds the task with `id:<id>` as a whole `key:value` word, first match. Mirrors the daemon's
/// own "first `id:`-word scan" (`crates/txtodo-daemon/src/fastid.rs`), reimplemented here for the
/// same crate-boundary reason as the rest of this module.
pub fn find_by_id<'a>(text: &'a str, id: &str) -> Option<(u32, &'a str)> {
    lines(text)
        .into_iter()
        .find(|(_, raw)| raw.split_whitespace().any(|w| w == format!("id:{id}")))
}

/// `txtodo list TERM...`'s matching, the todo.sh `filtercommand` semantics (root todo
/// id:01M2T868JD32M84JQQ2ABASXW4): `query` is split on whitespace, every term must match (AND), a
/// term is a case-insensitive substring of the line, and a term with a leading `-` (and something
/// after it) excludes lines containing the rest. An empty query keeps every line. Mirrors
/// `txtodo-cli`'s `commands::list::matches` — this crate may not depend on that binary, so the
/// vectors in the tests below are the same, and MCP search agrees with CLI search.
pub fn matches_query(raw: &str, query: &str) -> bool {
    let hay = raw.to_lowercase();
    query
        .split_whitespace()
        .all(|term| match term.strip_prefix('-') {
            Some(neg) if !neg.is_empty() => !hay.contains(&neg.to_lowercase()),
            _ => hay.contains(&term.to_lowercase()),
        })
}

/// A typed token a caller asked for by name: `todotxt://project/<name>`, `todotxt://context/<name>`
/// and `triage_inbox`'s context.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Token<'a> {
    /// `+name`.
    Project(&'a str),
    /// `@name`.
    Context(&'a str),
}

impl Token<'_> {
    /// The `todo_list` query that narrows the rows first. It is `matches_query`, a substring
    /// match, so it only ever keeps too much; [`Token::is_on`] decides.
    pub fn query(&self) -> String {
        match self {
            Token::Project(name) => format!("+{name}"),
            Token::Context(name) => format!("@{name}"),
        }
    }

    /// Whether `row` carries exactly this token (code review 2026-09-20, finding 7). A substring
    /// is not enough for a typed lookup: `project/work` must not return `+workshop`, `context/home`
    /// must not return a line holding `bob@home.com`, and `triage_inbox` must not pull `@inbox-old`.
    /// `todo_list` and `todo_search` keep the substring match, for parity with `txtodo list`.
    pub fn is_on(&self, row: &TaskRow) -> bool {
        match self {
            Token::Project(name) => row.projects.iter().any(|p| p == name),
            Token::Context(name) => row.contexts.iter().any(|c| c == name),
        }
    }
}

/// `raw` + (a space, unless `text` opens with a sentence delimiter) + `text` (todo.sh `append`).
pub fn append(raw: &str, text: &str) -> String {
    const SENTENCE_DELIMITERS: [char; 4] = [',', '.', ':', ';'];
    let sep = if text.starts_with(SENTENCE_DELIMITERS) {
        ""
    } else {
        " "
    };
    format!("{raw}{sep}{text}")
}

/// Old prefix (priority + date) kept, except that a priority or date at the start of `text`
/// replaces it and is then stripped from `text` (todo.sh `replaceOrPrepend`, `replace` direction).
pub fn replace_body(raw: &str, text: &str) -> String {
    let (p, d) = prefix_lens(raw);
    let (np, nd) = prefix_lens(text);
    let pri = if np > 0 { &text[..np] } else { &raw[..p] };
    let date = if nd > 0 {
        &text[np..np + nd]
    } else {
        &raw[p..p + d]
    };
    format!("{pri}{date}{}", &text[np + nd..])
}

/// Sets (`Some`) or clears (`None`) the leading `(X) ` priority prefix.
pub fn set_priority(raw: &str, priority: Option<char>) -> String {
    let (p, _) = prefix_lens(raw);
    let body = &raw[p..];
    match priority {
        Some(c) => format!("({c}) {body}"),
        None => body.to_owned(),
    }
}

/// Sets (`Some`) or removes (`None`) a `key:value` tag, replacing the first occurrence in place or
/// appending at the end when absent.
pub fn set_kv(raw: &str, key: &str, value: Option<&str>) -> String {
    let prefix = format!("{key}:");
    let words: Vec<&str> = raw.split(' ').collect();
    let idx = words.iter().position(|w| w.starts_with(&prefix));
    let mut out: Vec<String> = words.iter().map(|s| s.to_string()).collect();
    match (idx, value) {
        (Some(i), Some(v)) => out[i] = format!("{key}:{v}"),
        (Some(i), None) => {
            out.remove(i);
        }
        (None, Some(v)) => out.push(format!("{key}:{v}")),
        (None, None) => {}
    }
    out.join(" ")
}

/// Reopens a completed line: strips `x ` and its completion date, restoring `pri:X` (if any) to a
/// leading `(X) ` (todo.sh `pri:` preservation, in reverse) and dropping the `pri:` tag.
pub fn uncomplete_line(raw: &str) -> String {
    let row = parse_row(0, raw);
    if !row.done {
        return raw.to_owned();
    }
    let mut body = raw.strip_prefix("x ").unwrap_or(raw).to_owned();
    if let Some(completed) = &row.completed {
        let with_space = format!("{completed} ");
        if let Some(after) = body.strip_prefix(with_space.as_str()) {
            body = after.to_owned();
        }
    }
    if let Some(p) = row
        .kv
        .iter()
        .find(|(k, _)| k == "pri")
        .map(|(_, v)| v.clone())
    {
        body = set_kv(&body, "pri", None);
        body = format!("({p}) {body}");
    }
    body
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_priority_dates_projects_contexts_kv_and_id() {
        let row = parse_row(
            3,
            "(A) 2026-09-11 Draft +work @laptop due:2026-09-14 id:01J id:ignored",
        );
        assert_eq!(row.priority, Some('A'));
        assert_eq!(row.created.as_deref(), Some("2026-09-11"));
        assert_eq!(row.projects, vec!["work"]);
        assert_eq!(row.contexts, vec!["laptop"]);
        assert_eq!(row.due.as_deref(), Some("2026-09-14"));
        assert_eq!(row.id.as_deref(), Some("01J"));
        assert!(!row.done);
        assert_eq!(row.line, 3);
    }

    #[test]
    fn done_line_takes_completion_date_and_pri_tag_as_priority() {
        let row = parse_row(1, "x 2026-09-12 2026-09-11 Draft pri:B id:01J");
        assert!(row.done);
        assert_eq!(row.completed.as_deref(), Some("2026-09-12"));
        assert_eq!(row.created.as_deref(), Some("2026-09-11"));
        assert_eq!(row.priority, Some('B'));
    }

    #[test]
    fn find_by_id_matches_the_whole_word_only() {
        let text = "a id:01J one\nb id:01J99 two\n";
        let found = find_by_id(text, "01J");
        assert_eq!(found, Some((1, "a id:01J one")));
    }

    #[test]
    fn a_typed_token_matches_the_whole_word_not_a_substring() {
        let work = parse_row(1, "fix the door +work @home");
        let workshop = parse_row(2, "sand the bench +workshop mail bob@home.com @inbox-old");
        assert!(Token::Project("work").is_on(&work));
        assert!(
            !Token::Project("work").is_on(&workshop),
            "+workshop is not +work"
        );
        assert!(Token::Context("home").is_on(&work));
        assert!(
            !Token::Context("home").is_on(&workshop),
            "bob@home.com is not @home"
        );
        assert!(
            !Token::Context("inbox").is_on(&workshop),
            "@inbox-old is not @inbox"
        );
        // The narrowing query is the substring one, so it keeps both; `is_on` decides.
        assert_eq!(Token::Project("work").query(), "+work");
        assert!(matches_query(
            &workshop.raw,
            &Token::Project("work").query()
        ));
    }

    const QUERY_LINE: &str = "(A) 2026-09-11 Draft Milk +work @laptop id:01J";

    #[test]
    fn a_term_is_a_case_insensitive_substring_including_project_and_context() {
        assert!(matches_query(QUERY_LINE, "milk"));
        assert!(matches_query(QUERY_LINE, "+work"));
        assert!(matches_query(QUERY_LINE, "@LAPTOP"));
    }

    #[test]
    fn every_term_must_match() {
        assert!(matches_query(QUERY_LINE, "draft milk"));
        assert!(!matches_query(QUERY_LINE, "milk eggs"));
    }

    #[test]
    fn a_leading_dash_excludes_and_a_lone_dash_is_a_substring() {
        assert!(!matches_query(QUERY_LINE, "-milk"));
        assert!(matches_query(QUERY_LINE, "-eggs"));
        assert!(matches_query(QUERY_LINE, "draft -eggs"));
        assert!(matches_query("a - b", "-"));
    }

    #[test]
    fn no_terms_keeps_every_line() {
        assert!(matches_query(QUERY_LINE, ""));
        assert!(matches_query(QUERY_LINE, "   "));
    }

    #[test]
    fn append_and_replace_body_keep_todo_sh_shape() {
        assert_eq!(append("a", "b"), "a b");
        assert_eq!(append("a", ", b"), "a, b");
        assert_eq!(
            replace_body("(A) 2026-09-11 old", "new"),
            "(A) 2026-09-11 new"
        );
        assert_eq!(
            replace_body("(A) 2026-09-11 old", "(B) new"),
            "(B) 2026-09-11 new"
        );
    }

    #[test]
    fn set_priority_sets_and_clears() {
        assert_eq!(set_priority("plain task", Some('A')), "(A) plain task");
        assert_eq!(set_priority("(A) plain task", None), "plain task");
        assert_eq!(set_priority("(A) plain task", Some('B')), "(B) plain task");
    }

    #[test]
    fn set_kv_sets_replaces_and_removes() {
        assert_eq!(
            set_kv("task", "due", Some("2026-09-14")),
            "task due:2026-09-14"
        );
        assert_eq!(
            set_kv("task due:2026-09-14", "due", Some("2026-09-20")),
            "task due:2026-09-20"
        );
        assert_eq!(set_kv("task due:2026-09-14", "due", None), "task");
    }

    #[test]
    fn uncomplete_restores_priority_from_pri_tag() {
        let restored = uncomplete_line("x 2026-09-12 2026-09-11 Draft pri:B id:01J");
        assert_eq!(restored, "(B) 2026-09-11 Draft id:01J");
    }

    #[test]
    fn uncomplete_is_a_no_op_on_an_open_task() {
        assert_eq!(uncomplete_line("(A) open task"), "(A) open task");
    }
}
