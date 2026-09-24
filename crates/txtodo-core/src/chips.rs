//! The prompt bar's chips (task `tui-revamp/shared-core`): tap `(A)` `(B)` `(C)` to set or clear
//! a priority, `x` to complete or reopen, `+` `@` `due:` `t:` `rec:` to insert a token at the
//! caret. Ported from desktop's `editPopoverLogic.ts` (`applyChip`, `toggleComplete`), whose
//! tests are mirrored below, so the TUI and desktop (through `txtodo-ffi`) edit the same way.
//! Carets here are byte offsets into the line; the wasm shim converts from and to UTF-16.
//! Grammar: `specs/todotxt.abnf` (`priority`, `completed`/`incomplete`, `pri-tag`).

use alloc::string::String;
use alloc::vec::Vec;
use alloc::{format, vec};

/// One chip on the prompt bar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Chip {
    /// `(A)`, `(B)` or `(C)`: set this priority, or clear it when the line already has it.
    Priority(char),
    /// `x`: complete the line, or reopen a completed one.
    Complete,
    /// `+`, `@`, `due:`, `t:` or `rec:`: insert the token at the caret.
    Token(&'static str),
}

impl Chip {
    /// The chip named the way desktop names it: `A` `B` `C` `x` `+` `@` `due:` `t:` `rec:`.
    pub fn parse(name: &str) -> Option<Chip> {
        match name {
            "A" | "B" | "C" => name.chars().next().map(Chip::Priority),
            "x" => Some(Chip::Complete),
            "+" => Some(Chip::Token("+")),
            "@" => Some(Chip::Token("@")),
            "due:" => Some(Chip::Token("due:")),
            "t:" => Some(Chip::Token("t:")),
            "rec:" => Some(Chip::Token("rec:")),
            _ => None,
        }
    }
}

/// Applies one chip at byte offset `caret` in `raw`: the new text and where the caret lands.
/// `today` (`YYYY-MM-DD`, local calendar, ADR 0011) is only read by [`Chip::Complete`].
pub fn apply_chip(raw: &str, caret: usize, chip: Chip, today: &str) -> (String, usize) {
    let caret = floor_boundary(raw, caret);
    match chip {
        Chip::Priority(letter) => toggle_priority(raw, caret, letter),
        Chip::Complete => {
            let text = toggle_complete_text(raw, today);
            let end = text.len();
            (text, end)
        }
        Chip::Token(token) => insert_chip(raw, caret, token),
    }
}

/// The largest char boundary at or below `at`, clamped to `raw`: a caret never splits a char.
fn floor_boundary(raw: &str, at: usize) -> usize {
    let mut at = at.min(raw.len());
    // Bounded: at most three steps back to a UTF-8 boundary.
    while !raw.is_char_boundary(at) {
        at -= 1;
    }
    at
}

/// `(X) ` or a bare `(X)` at the start: the letter and the rest.
fn strip_priority(raw: &str) -> Option<(char, &str)> {
    let b = raw.as_bytes();
    let [b'(', letter, b')', ..] = b else {
        return None;
    };
    if !letter.is_ascii_uppercase() {
        return None;
    }
    let rest = &raw[3..];
    Some((char::from(*letter), rest.strip_prefix(' ').unwrap_or(rest)))
}

/// Sets `letter`, or clears it when the line already has it. Only the `(X) ` prefix changes, so
/// a caret at or after it moves by the prefix's change in length; one inside a removed prefix
/// clamps to the start.
pub fn toggle_priority(raw: &str, caret: usize, letter: char) -> (String, usize) {
    let (current, rest) = match strip_priority(raw) {
        Some((l, rest)) => (Some(l), rest),
        None => (None, raw),
    };
    let text = if current == Some(letter) {
        String::from(rest)
    } else if rest.is_empty() {
        format!("({letter})")
    } else {
        format!("({letter}) {rest}")
    };
    let old_prefix = raw.len() - rest.len();
    let new_prefix = text.len() - rest.len();
    let moved = (caret + new_prefix).saturating_sub(old_prefix);
    let caret = floor_boundary(&text, moved);
    (text, caret)
}

/// Inserts `token` at `caret`, with one space before it unless the caret is at the start or
/// after whitespace already (no double spaces).
pub fn insert_chip(raw: &str, caret: usize, token: &str) -> (String, usize) {
    let caret = floor_boundary(raw, caret);
    let (before, after) = raw.split_at(caret);
    let at_boundary = before.chars().next_back().is_none_or(char::is_whitespace);
    let insert = if at_boundary {
        String::from(token)
    } else {
        format!(" {token}")
    };
    let end = before.len() + insert.len();
    (format!("{before}{insert}{after}"), end)
}

/// `YYYY-MM-DD` at the start of `s`.
fn is_date(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() >= 10
        && b[..4].iter().all(u8::is_ascii_digit)
        && b[4] == b'-'
        && b[5..7].iter().all(u8::is_ascii_digit)
        && b[7] == b'-'
        && b[8..10].iter().all(u8::is_ascii_digit)
}

/// `date [" " rest]` covering all of `s`: the date and the rest (`""` when absent). `None` when
/// something other than a space follows the date.
fn split_date(s: &str) -> Option<(&str, &str)> {
    if !is_date(s) {
        return None;
    }
    let (date, tail) = s.split_at(10);
    match tail {
        "" => Some((date, "")),
        _ => tail.strip_prefix(' ').map(|rest| (date, rest)),
    }
}

/// `x <done> [<created>] [<description>]` covering the whole line: the creation date and the
/// description. `None` for a line that is not completed in that exact shape.
fn split_completed(raw: &str) -> Option<(Option<&str>, &str)> {
    let (_done, rest) = split_date(raw.strip_prefix("x ")?)?;
    match split_date(rest) {
        Some((created, description)) => Some((Some(created), description)),
        None => Some((None, rest)),
    }
}

/// Completes `raw` (`x <today> [created] description [pri:P]`, the priority kept as a `pri:` tag
/// per `core-complete-pri`) or reopens a completed line (the reverse, the first `pri:P` back as a
/// leading `(P)`). Plain text in, plain text out.
pub fn toggle_complete_text(raw: &str, today: &str) -> String {
    if let Some((created, description)) = split_completed(raw) {
        return reopen(created, description);
    }
    let (priority, rest) = match strip_priority(raw) {
        Some((l, rest)) => (Some(l), rest),
        None => (None, raw),
    };
    let (created, description) = match split_date(rest) {
        Some((date, description)) => (Some(date), description),
        None => (None, rest),
    };
    let mut parts: Vec<String> = vec![String::from("x"), String::from(today)];
    parts.extend(created.map(String::from));
    if !description.is_empty() {
        parts.push(String::from(description));
    }
    parts.extend(priority.map(|p| format!("pri:{p}")));
    parts.join(" ")
}

/// The reopen half of [`toggle_complete_text`]: words split on single spaces, as the desktop does,
/// so a double space survives the round trip.
fn reopen(created: Option<&str>, description: &str) -> String {
    let mut priority: Option<char> = None;
    let words: Vec<&str> = if description.is_empty() {
        Vec::new()
    } else {
        description.split(' ').collect()
    };
    let kept: Vec<&str> = words
        .into_iter()
        .filter(|w| {
            if priority.is_none()
                && let Some(p) = pri_tag(w)
            {
                priority = Some(p);
                return false;
            }
            true
        })
        .collect();
    let mut parts: Vec<String> = Vec::new();
    parts.extend(priority.map(|p| format!("({p})")));
    parts.extend(created.map(String::from));
    if !kept.is_empty() {
        parts.push(kept.join(" "));
    }
    parts.join(" ")
}

/// `pri:X` with one upper-case letter.
fn pri_tag(word: &str) -> Option<char> {
    match word.as_bytes() {
        [b'p', b'r', b'i', b':', p] if p.is_ascii_uppercase() => Some(char::from(*p)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TODAY: &str = "2026-09-12";

    fn chip(raw: &str, caret: usize, name: &str) -> (String, usize) {
        let chip = Chip::parse(name).unwrap_or_else(|| panic!("chip {name}"));
        apply_chip(raw, caret, chip, TODAY)
    }

    /// `apps/desktop/src/lib/components/__tests__/editPopoverLogic.test.ts`, case for case.
    #[test]
    fn matches_the_desktop_chip_tests() {
        let s = String::from;
        assert_eq!(chip("call mum", 0, "A"), (s("(A) call mum"), 4));
        assert_eq!(chip("(A) call mum", 0, "A"), (s("call mum"), 0));
        assert_eq!(chip("(B) call mum", 5, "A"), (s("(A) call mum"), 5));
        assert_eq!(chip("(A)", 0, "A"), (s(""), 0));
        assert_eq!(chip("call mum ", 9, "+"), (s("call mum +"), 10));
        assert_eq!(chip("call mum", 8, "+"), (s("call mum +"), 10));
        assert_eq!(chip("call mum", 4, "@"), (s("call @ mum"), 6));
        assert_eq!(chip("call  mum", 5, "due:"), (s("call due: mum"), 9));
        let done = toggle_complete_text("call mum", TODAY);
        assert_eq!(chip("call mum", 0, "x"), (done.clone(), done.len()));
    }

    #[test]
    fn complete_and_reopen_round_trip() {
        assert_eq!(
            toggle_complete_text("call mum", TODAY),
            "x 2026-09-12 call mum"
        );
        assert_eq!(
            toggle_complete_text("x 2026-09-12 call mum", TODAY),
            "call mum"
        );
        let original = "(A) 2026-09-01 call mum +home";
        let completed = toggle_complete_text(original, TODAY);
        assert_eq!(completed, "x 2026-09-12 2026-09-01 call mum +home pri:A");
        assert_eq!(toggle_complete_text(&completed, TODAY), original);
    }

    /// The c2 mockup's self-test (`design-mockups/shared/selftest.mjs:72-77`).
    #[test]
    fn matches_the_mockup_selftest() {
        let s = String::from;
        let today = "2026-09-19";
        assert_eq!(
            toggle_complete_text("(B) 2026-09-17 buy milk", today),
            "x 2026-09-19 2026-09-17 buy milk pri:B"
        );
        assert_eq!(
            toggle_complete_text("x 2026-09-19 2026-09-17 buy milk pri:B", today),
            "(B) 2026-09-17 buy milk"
        );
        assert_eq!(chip("(B) buy milk", 0, "A").0, s("(A) buy milk"));
        assert_eq!(chip("(A) buy milk", 4, "A").0, s("buy milk"));
        assert_eq!(chip("buy milk ", 9, "@").0, s("buy milk @"));
    }

    #[test]
    fn a_caret_never_splits_a_char_and_odd_lines_stay_whole() {
        // "é" is two bytes: a caret of 1 falls back to 0.
        assert_eq!(insert_chip("é", 1, "+"), (String::from("+é"), 1));
        assert_eq!(toggle_priority("(a) lower", 0, 'A').0, "(A) (a) lower");
        assert_eq!(
            toggle_complete_text("x 2026-09-12x", TODAY),
            "x 2026-09-12 x 2026-09-12x"
        );
        assert_eq!(Chip::parse("D"), None);
    }
}
