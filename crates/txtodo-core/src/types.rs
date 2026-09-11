//! Core value types of the M1 API: modes, spans, dates, priorities, lines, tasks, files.


use core::fmt;

/// How strictly to read a line. `Strict` enforces `specs/todotxt.abnf`; `Lenient` records deviations as quirks.
/// Writing is always strict; reading defaults to lenient (design §2.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// Any deviation from the grammar is a [`crate::ParseError`].
    Strict,
    /// Deviations become quirks; never fails for a `&str`.
    Lenient,
}

/// A classified byte range of a line. Spans from [`tokenize`](crate) are contiguous and cover every byte.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Span {
    /// What the bytes are.
    pub kind: TokenKind,
    /// Start byte offset, inclusive; always a UTF-8 char boundary.
    pub start: usize,
    /// End byte offset, exclusive; always a UTF-8 char boundary.
    pub end: usize,
}

/// Token classes. Names match `corpus/tokens.schema.json` exactly; highlighters map them to theme colours.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TokenKind {
    /// The leading lowercase `x`.
    CompletionMarker,
    /// The date right after `x`.
    CompletionDate,
    /// The creation date (after the completion date, or after the priority, or first).
    CreationDate,
    /// `(A)` through `(Z)`.
    Priority,
    /// `+project`.
    Project,
    /// `@context`.
    Context,
    /// `key:` of a `key:value` tag (colon included).
    TagKey,
    /// `value` of a `key:value` tag.
    TagValue,
    /// A whole `id:<ULID>` tag.
    IdTag,
    /// A word starting with a recognised URL scheme and a colon.
    Url,
    /// Anything else that is not whitespace.
    Text,
    /// A run of ASCII spaces and/or tabs.
    Whitespace,
}

/// A calendar date, `YYYY-MM-DD`, validated (month 1–12, day within the month, leap years). No time zone: ADR 0011.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Date {
    year: u16,
    month: u8,
    day: u8,
}

impl Date {
    /// Builds a date if it exists on the proleptic Gregorian calendar. Years are 0000–9999 (four digits).
    pub fn new(year: u16, month: u8, day: u8) -> Option<Date> {
        if year > 9999 || month == 0 || month > 12 || day == 0 || day > days_in_month(year, month) {
            return None;
        }
        let date = Date { year, month, day };
        debug_assert!(date.day <= 31, "day bounded by days_in_month");
        Some(date)
    }

    /// Parses exactly `YYYY-MM-DD` (ten ASCII bytes) and validates the calendar. Anything else is `None`.
    pub fn parse(s: &str) -> Option<Date> {
        let b = s.as_bytes();
        if b.len() != 10 || b[4] != b'-' || b[7] != b'-' {
            return None;
        }
        let year = ascii_number(&b[0..4])?;
        let month = ascii_number(&b[5..7])?;
        let day = ascii_number(&b[8..10])?;
        debug_assert!(year <= 9999, "four digits cannot exceed 9999");
        Date::new(year, month as u8, day as u8)
    }

    /// Year, 0–9999.
    pub fn year(self) -> u16 {
        self.year
    }
    /// Month, 1–12.
    pub fn month(self) -> u8 {
        self.month
    }
    /// Day of month, 1–31.
    pub fn day(self) -> u8 {
        self.day
    }
}

impl fmt::Display for Date {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }
}

/// Days in `month` of `year`, Gregorian rules. `month` must be 1–12.
fn days_in_month(year: u16, month: u8) -> u8 {
    debug_assert!((1..=12).contains(&month), "caller validated month");
    let leap = year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400));
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        _ => 28,
    }
}

/// Parses 1–4 ASCII digits; `None` on any non-digit.
fn ascii_number(digits: &[u8]) -> Option<u16> {
    debug_assert!(!digits.is_empty() && digits.len() <= 4, "callers pass 2 or 4 digits");
    let mut n: u16 = 0;
    for &d in digits {
        if !d.is_ascii_digit() {
            return None;
        }
        n = n * 10 + u16::from(d - b'0');
    }
    Some(n)
}

/// A priority `(A)`–`(Z)`. Stored as the uppercase ASCII letter.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Priority(u8);

impl Priority {
    /// `Some` for `'A'..='Z'` only; lowercase is not a priority (design §2.4).
    pub fn new(letter: char) -> Option<Priority> {
        if letter.is_ascii_uppercase() {
            Some(Priority(letter as u8))
        } else {
            None
        }
    }
    /// The letter, `'A'..='Z'`.
    pub fn as_char(self) -> char {
        debug_assert!(self.0.is_ascii_uppercase(), "constructor guarantees uppercase");
        char::from(self.0)
    }
}

impl fmt::Display for Priority {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "({})", self.as_char())
    }
}

/// How a line ends on disk. Preserved per line; never canonicalised as a side effect (design §2.2 rule 7).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum LineEnding {
    /// `\n`.
    #[default]
    Lf,
    /// `\r\n`.
    CrLf,
    /// No newline: only possible on the last line of a file.
    None,
}

impl LineEnding {
    /// The bytes written after the line.
    pub fn as_bytes(self) -> &'static [u8] {
        match self {
            LineEnding::Lf => b"\n",
            LineEnding::CrLf => b"\r\n",
            LineEnding::None => b"",
        }
    }
}

/// One parsed line, borrowing from its source. `raw` is the line without its ending.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Line<'a> {
    /// The line bytes exactly as read, minus the line ending.
    pub raw: &'a str,
    /// Blank or task.
    pub kind: LineKind<'a>,
    /// Leniencies recorded while parsing (empty in strict mode).
    pub quirks: crate::Quirks,
    /// The ending this line had (or will get).
    pub ending: LineEnding,
}

/// What a line is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LineKind<'a> {
    /// An empty line. Blank lines are entries so line numbers stay stable (design §2.2 rule 6).
    Blank,
    /// A task line.
    Task(Task<'a>),
}

/// The structured prefix of a task plus its verbatim description. Projects, contexts and tags are views
/// computed from `description` on demand (design §3), never stored.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Task<'a> {
    /// Line started with `x `.
    pub completed: bool,
    /// Date right after `x`, if any.
    pub completion_date: Option<Date>,
    /// Creation date, if any.
    pub creation_date: Option<Date>,
    /// `(A)`–`(Z)`, if any.
    pub priority: Option<Priority>,
    /// Everything after the prefix, byte for byte.
    pub description: &'a str,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn leap_years_follow_gregorian_rules() {
        // (year, feb 29 valid?) — 2000 is a leap year, 1900 and 2100 are not, 2024 is.
        for (year, ok) in [(2000, true), (1900, false), (2100, false), (2024, true), (2023, false)] {
            assert_eq!(Date::new(year, 2, 29).is_some(), ok, "year {year}");
        }
    }

    #[test]
    fn parse_accepts_only_exact_shape_and_real_days() {
        assert_eq!(Date::parse("2026-09-11"), Date::new(2026, 9, 11));
        for bad in ["2026-9-11", "2026-09-31", "2026-13-01", "2026-00-10", "26-09-11", "2026-09-11x", "2026/09/11"] {
            assert_eq!(Date::parse(bad), None, "{bad}");
        }
    }

    #[test]
    fn date_displays_zero_padded() {
        assert_eq!(alloc::format!("{}", Date::new(2026, 1, 2).unwrap()), "2026-01-02");
    }

    #[test]
    fn priority_is_uppercase_ascii_only() {
        assert_eq!(Priority::new('A').map(Priority::as_char), Some('A'));
        assert_eq!(Priority::new('a'), None);
        assert_eq!(Priority::new('É'), None);
        assert_eq!(alloc::format!("{}", Priority::new('Z').unwrap()), "(Z)");
    }

}
