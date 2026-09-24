//! The prompt bar's strict-mode hint (task `tui-revamp/shared-core`): one short message for the
//! first obvious grammar slip in a line being typed, or `None`. It never blocks a save (design
//! §2.3: lenient in, strict out); it only says what the strict grammar would refuse. Ported from
//! the c2 mockup's `strictHint` (`apps/desktop/design-mockups/shared/todotxt.js:308-313`) so the
//! TUI and desktop show the same words for the same text.
//! Ref: <https://developer.mozilla.org/en-US/docs/Web/JavaScript/Guide/Regular_expressions/Assertions>
//! (`\b`, which the date rule below reproduces by hand: core carries no regex engine).

/// A lower-case priority letter: `(a) call mum`.
pub const LOWERCASE_PRIORITY: &str = "Priority letters are uppercase: (A)–(Z).";
/// A leading `(` that is not one priority letter: `(AB) x`, `(1) x`, `( A) x`.
pub const NOT_A_PRIORITY: &str = "A leading (…) must be a single priority letter.";
/// A `due:` or `t:` tag whose value is not a `YYYY-MM-DD` date.
pub const DATE_TAG: &str = "due: and t: take a date, YYYY-MM-DD.";

/// The first hint `line` earns, in the mockup's order, or `None`.
pub fn strict_hint(line: &str) -> Option<&'static str> {
    let b = line.as_bytes();
    if let [b'(', letter, b')', ..] = b
        && letter.is_ascii_lowercase()
    {
        return Some(LOWERCASE_PRIORITY);
    }
    let one_letter = matches!(b, [b'(', letter, b')', ..] if letter.is_ascii_uppercase());
    if b.first() == Some(&b'(') && !one_letter {
        return Some(NOT_A_PRIORITY);
    }
    has_bad_date_tag(line).then_some(DATE_TAG)
}

/// A JS regex word character, `[A-Za-z0-9_]`.
fn is_word(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_'
}

/// `\b(?:due|t):(?!\d{4}-\d{2}-\d{2}\b)`: a `due:` or `t:` that starts a word and is not followed
/// by a whole `YYYY-MM-DD`.
fn has_bad_date_tag(line: &str) -> bool {
    let b = line.as_bytes();
    // Bounded by the line length.
    (0..b.len()).any(|i| {
        let at_boundary = i == 0 || !is_word(b[i - 1]);
        let key_len = [&b"due:"[..], &b"t:"[..]]
            .into_iter()
            .find(|key| b[i..].starts_with(key))
            .map(<[u8]>::len);
        match key_len {
            Some(n) if at_boundary => !is_date_word(&b[i + n..]),
            _ => false,
        }
    })
}

/// `\d{4}-\d{2}-\d{2}\b` at the start of `rest`.
fn is_date_word(rest: &[u8]) -> bool {
    let shape = rest.len() >= 10
        && rest[..4].iter().all(u8::is_ascii_digit)
        && rest[4] == b'-'
        && rest[5..7].iter().all(u8::is_ascii_digit)
        && rest[7] == b'-'
        && rest[8..10].iter().all(u8::is_ascii_digit);
    shape && rest.get(10).is_none_or(|c| !is_word(*c))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_the_mockup_regexes_on_a_table() {
        let table: &[(&str, Option<&str>)] = &[
            ("(a) nope", Some(LOWERCASE_PRIORITY)),
            ("(A) ok due:2026-09-26", None),
            ("(AB) two letters", Some(NOT_A_PRIORITY)),
            ("(1) a digit", Some(NOT_A_PRIORITY)),
            ("( A) a space", Some(NOT_A_PRIORITY)),
            ("(", Some(NOT_A_PRIORITY)),
            ("plain line", None),
            ("x (a) done lines are left alone", None),
            ("pay rent due:tomorrow", Some(DATE_TAG)),
            ("pay rent due:2026-9-1", Some(DATE_TAG)),
            ("pay rent due:2026-09-01x", Some(DATE_TAG)),
            ("pay rent due:2026-09-01, then relax", None),
            ("start t:2026-10-01", None),
            ("start t:soon", Some(DATE_TAG)),
            ("meet at:home", None),
            ("a bare t: still counts", Some(DATE_TAG)),
            ("due:", Some(DATE_TAG)),
            (
                "(a) due:x reports the priority first",
                Some(LOWERCASE_PRIORITY),
            ),
        ];
        for (line, want) in table {
            assert_eq!(strict_hint(line), *want, "{line:?}");
        }
    }
}
