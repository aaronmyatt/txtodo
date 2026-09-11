//! `parse_line`: the hand-written recursive-descent parser for `specs/todotxt.abnf`.
//! Strict mode errors on any grammar deviation; lenient mode records the named leniencies as quirks and
//! is total over `&str` (never `Err`). A second, ABNF-generated parser checks this one in tests.

use crate::scanner::{chunks, Chunk};
use crate::task::is_valid_slug;
use crate::tokenize::is_priority_word;
use crate::urls::DEFAULT_SCHEMES;
use crate::{Date, Line, LineEnding, LineKind, Mode, ParseError, Priority, Quirks, Task};
use alloc::vec::Vec;

/// Parses one line (without its ending) with [`DEFAULT_SCHEMES`]. `""` is [`LineKind::Blank`].
/// The returned `ending` is the default; [`crate::File`] knows the real one.
pub fn parse_line(raw: &str, mode: Mode) -> Result<Line<'_>, ParseError> {
    parse_line_with_schemes(raw, mode, DEFAULT_SCHEMES)
}

/// [`parse_line`] with a custom URL scheme list (only affects the `INVALID_REF` check on tags).
pub fn parse_line_with_schemes<'a>(raw: &'a str, mode: Mode, schemes: &[&str]) -> Result<Line<'a>, ParseError> {
    if raw.is_empty() {
        return Ok(Line { raw, kind: LineKind::Blank, quirks: Quirks::NONE, ending: LineEnding::default() });
    }
    let mut p = Parser { raw, chunks: chunks(raw).collect(), i: 0, mode, quirks: Quirks::NONE };
    let task = p.task()?;
    if task.tag_with_schemes("ref", schemes).is_some_and(|slug| !is_valid_slug(slug)) {
        p.quirks.insert(Quirks::INVALID_REF);
    }
    debug_assert!(mode == Mode::Lenient || p.quirks == Quirks::NONE || p.quirks == Quirks::INVALID_REF, "strict has no leniency quirks");
    Ok(Line { raw, kind: LineKind::Task(task), quirks: p.quirks, ending: LineEnding::default() })
}

/// Cursor over the scanner's chunks plus the quirks collected so far.
struct Parser<'a> {
    raw: &'a str,
    chunks: Vec<Chunk>,
    i: usize,
    mode: Mode,
    quirks: Quirks,
}

impl<'a> Parser<'a> {
    /// The current chunk as a word, or `None` at end / on whitespace.
    fn word(&self) -> Option<&'a str> {
        let c = self.chunks.get(self.i)?;
        (!c.is_ws).then(|| &self.raw[c.start..c.end])
    }

    /// Byte offset of the current chunk, or the line length at end.
    fn pos(&self) -> usize {
        self.chunks.get(self.i).map_or(self.raw.len(), |c| c.start)
    }

    /// In lenient mode records `quirk` and continues; in strict mode fails with `rule` at the cursor.
    fn lenient_or(&mut self, quirk: Quirks, rule: &'static str, message: &'static str) -> Result<(), ParseError> {
        let byte = self.pos();
        self.lenient_or_at(quirk, rule, message, byte)
    }

    /// [`Parser::lenient_or`] with an explicit error offset.
    fn lenient_or_at(&mut self, quirk: Quirks, rule: &'static str, message: &'static str, byte: usize) -> Result<(), ParseError> {
        debug_assert!(byte <= self.raw.len(), "offset within the line");
        if self.mode == Mode::Lenient {
            self.quirks.insert(quirk);
            return Ok(());
        }
        Err(ParseError::new(rule, byte, message))
    }

    /// Consumes the whitespace chunk after a prefix element. `Ok(true)` when more words follow.
    /// Strict: the separator must be exactly one space and must not be trailing.
    fn separator(&mut self) -> Result<bool, ParseError> {
        let Some(c) = self.chunks.get(self.i).copied() else {
            return Ok(false);
        };
        debug_assert!(c.is_ws, "prefix elements are words; the next chunk is whitespace or end");
        if &self.raw[c.start..c.end] != " " {
            self.lenient_or(Quirks::TABS, "SP", "words are separated by exactly one space")?;
        }
        self.i += 1;
        if self.i == self.chunks.len() {
            self.lenient_or_at(Quirks::TRAILING_WS, "description", "trailing whitespace", c.start)?;
            return Ok(false);
        }
        Ok(true)
    }

    /// Whole task: prefix, then the verbatim remainder as description.
    fn task(&mut self) -> Result<Task<'a>, ParseError> {
        let mut t = Task { completed: false, completion_date: None, creation_date: None, priority: None, description: "" };
        if self.word() == Some("x") {
            self.i += 1;
            t.completed = true;
            self.completed_prefix(&mut t)?;
        } else {
            self.incomplete_prefix(&mut t)?;
        }
        t.description = self.description()?;
        Ok(t)
    }

    /// After `x`: `SP date [SP date]`, with lenient priority placements.
    fn completed_prefix(&mut self, t: &mut Task<'a>) -> Result<(), ParseError> {
        if !self.separator()? {
            return self.lenient_or(Quirks::NO_COMPLETION_DATE, "completed", "expected a space and the completion date after x");
        }
        if self.mode == Mode::Lenient && self.take_priority(t)? {
            self.quirks.insert(Quirks::PRIORITY_AFTER_X);
        }
        match self.take_date()? {
            Some(d) => t.completion_date = Some(d),
            None => return self.lenient_or(Quirks::NO_COMPLETION_DATE, "completed", "expected the completion date after x"),
        }
        if self.separator()? {
            self.take_optional_date(&mut t.creation_date)?;
        }
        if self.mode == Mode::Lenient && t.priority.is_none() && self.take_priority(t)? {
            self.quirks.insert(Quirks::PRIORITY_AFTER_DATE);
        }
        Ok(())
    }

    /// `[priority SP] [date SP]`.
    fn incomplete_prefix(&mut self, t: &mut Task<'a>) -> Result<(), ParseError> {
        self.take_priority(t)?;
        self.take_optional_date(&mut t.creation_date)
    }

    /// Takes `(A)` and its separator if present. `Ok(true)` when a priority was taken.
    fn take_priority(&mut self, t: &mut Task<'a>) -> Result<bool, ParseError> {
        let Some(word) = self.word().filter(|w| is_priority_word(w)) else {
            return Ok(false);
        };
        t.priority = Priority::new(char::from(word.as_bytes()[1]));
        debug_assert!(t.priority.is_some(), "is_priority_word guarantees A-Z");
        self.i += 1;
        self.separator()?;
        Ok(true)
    }

    /// Takes a date word if the current word is one. A date-shaped word with an impossible calendar date is
    /// a strict error (rule `date`) and plain text in lenient mode.
    fn take_date(&mut self) -> Result<Option<Date>, ParseError> {
        let Some(word) = self.word() else {
            return Ok(None);
        };
        match Date::parse(word) {
            Some(d) => {
                self.i += 1;
                Ok(Some(d))
            }
            None if self.mode == Mode::Strict && has_date_shape(word) => {
                Err(ParseError::new("date", self.pos(), "not a calendar date"))
            }
            None => Ok(None),
        }
    }

    /// Optional date plus its separator.
    fn take_optional_date(&mut self, slot: &mut Option<Date>) -> Result<(), ParseError> {
        if let Some(d) = self.take_date()? {
            *slot = Some(d);
            self.separator()?;
        }
        Ok(())
    }

    /// Everything from the current chunk on, after checking separators inside it.
    fn description(&mut self) -> Result<&'a str, ParseError> {
        let start = self.pos();
        let rest: Vec<Chunk> = self.chunks[self.i..].to_vec();
        for (n, c) in rest.iter().enumerate() {
            self.i += 1;
            if !c.is_ws {
                continue;
            }
            let trailing = n + 1 == rest.len();
            let single = &self.raw[c.start..c.end] == " ";
            if trailing {
                self.lenient_or_at(Quirks::TRAILING_WS, "description", "trailing whitespace", c.start)?;
            } else if !single {
                self.lenient_or_at(Quirks::TABS, "SP", "words are separated by exactly one space", c.start)?;
            }
        }
        Ok(&self.raw[start..])
    }
}

/// `dddd-dd-dd` by shape, valid or not.
fn has_date_shape(word: &str) -> bool {
    let b = word.as_bytes();
    b.len() == 10 && b[4] == b'-' && b[7] == b'-' && b.iter().enumerate().all(|(i, &c)| i == 4 || i == 7 || c.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn task(raw: &str, mode: Mode) -> (Task<'_>, Quirks) {
        let line = parse_line(raw, mode).unwrap();
        match line.kind {
            LineKind::Task(t) => (t, line.quirks),
            LineKind::Blank => panic!("blank"),
        }
    }
    fn strict_err(raw: &str) -> ParseError {
        parse_line(raw, Mode::Strict).unwrap_err()
    }

    #[test]
    fn strict_prefixes() {
        let (t, q) = task("(A) 2026-09-11 Call +house due:2026-09-15", Mode::Strict);
        assert_eq!((t.priority.map(Priority::as_char), t.creation_date, t.description), (Some('A'), Date::new(2026, 9, 11), "Call +house due:2026-09-15"));
        assert!(q.is_empty());
        let (t, _) = task("x 2026-09-11 2026-09-01 Renew", Mode::Strict);
        assert_eq!((t.completed, t.completion_date, t.creation_date, t.description), (true, Date::new(2026, 9, 11), Date::new(2026, 9, 1), "Renew"));
        let (t, _) = task("x 2026-09-11 (A) task", Mode::Strict);
        assert_eq!((t.priority, t.description), (None, "(A) task"), "strict: a priority after the date is description text");
        assert_eq!(parse_line("", Mode::Strict).unwrap().kind, LineKind::Blank);
    }

    #[test]
    fn strict_errors_name_rule_and_byte() {
        assert_eq!(strict_err("x  2026-09-11 t"), ParseError::new("SP", 1, "words are separated by exactly one space"));
        assert_eq!(strict_err("x"), ParseError::new("completed", 1, "expected a space and the completion date after x"));
        assert_eq!(strict_err("x (A) 2026-09-11 t").rule, "completed");
        assert_eq!(strict_err("(A)  task"), ParseError::new("SP", 3, "words are separated by exactly one space"));
        assert_eq!(strict_err("2026-02-30 t"), ParseError::new("date", 0, "not a calendar date"));
        assert_eq!(strict_err("a\tb").rule, "SP");
        assert_eq!(strict_err("task "), ParseError::new("description", 4, "trailing whitespace"));
        assert_eq!(strict_err("(A) task  "), ParseError::new("description", 8, "trailing whitespace"));
    }

    #[test]
    fn lenient_records_quirks_and_never_fails() {
        let (t, q) = task("x no completion date here", Mode::Lenient);
        assert_eq!((t.completed, t.completion_date, t.description), (true, None, "no completion date here"));
        assert_eq!(q, Quirks::NO_COMPLETION_DATE);
        let (t, q) = task("x (A) 2026-09-11 priority after x", Mode::Lenient);
        assert_eq!((t.priority.map(Priority::as_char), t.completion_date.is_some()), (Some('A'), true));
        assert_eq!(q, Quirks::PRIORITY_AFTER_X);
        let (t, q) = task("x 2026-09-11 (A) priority after date", Mode::Lenient);
        assert_eq!((t.priority.map(Priority::as_char), t.description), (Some('A'), "priority after date"));
        assert_eq!(q, Quirks::PRIORITY_AFTER_DATE);
    }

    #[test]
    fn lenient_whitespace_and_shape_quirks() {
        let (t, q) = task("2026-09-11\ttab\twords", Mode::Lenient);
        assert_eq!((t.creation_date.is_some(), t.description, q), (true, "tab\twords", Quirks::TABS));
        let (_, q) = task("2026-09-11 t   ", Mode::Lenient);
        assert_eq!(q, Quirks::TRAILING_WS);
        let (t, q) = task("x 2026-09-11", Mode::Lenient);
        assert_eq!((t.completion_date.is_some(), t.description, q), (true, "", Quirks::NONE), "empty description is allowed");
        let (t, q) = task("2026-02-30 t", Mode::Lenient);
        assert_eq!((t.creation_date, t.description, q), (None, "2026-02-30 t", Quirks::NONE));
        let (_, q) = task("Bad ref:../x", Mode::Lenient);
        assert_eq!(q, Quirks::INVALID_REF);
    }

    #[test]
    fn design_2_4_non_errors() {
        let (t, _) = task("X 2026-09-11 not done", Mode::Strict);
        assert_eq!((t.completed, t.creation_date, t.description), (false, None, "X 2026-09-11 not done"));
        let (t, _) = task("(a) task", Mode::Strict);
        assert_eq!((t.priority, t.description), (None, "(a) task"));
        let (t, _) = task("+project at start", Mode::Strict);
        assert_eq!(t.description, "+project at start");
    }
}
