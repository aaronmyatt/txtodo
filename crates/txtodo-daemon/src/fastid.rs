//! A fast `id:` scan for the reconciler's hot path: the first whitespace-delimited word starting
//! with `id:` **that has a value** decides, exactly as `Task::id()` does after a full parse —
//! that equivalence is a property test below. Ten thousand full parses cost ~4.5 ms; this scan
//! costs a fraction.
//!
//! "That has a value" matters: `specs/todotxt.abnf`'s `tag = key ":" 1*NONSP` requires at least
//! one non-space character after the colon, so a bare `id:` immediately followed by whitespace
//! (e.g. prose that mentions the `id:` tag by name, like this project's own todo.txt) is not a
//! tag at all to the real parser — it's an ordinary word, and the parser keeps looking. A version
//! of this scan that stopped at the first `id:`-prefixed word regardless of whether it had a
//! value would treat that prose as "the id tag with an empty value" and give up right there,
//! diverging from `Task::id()` on any line where a real `id:<ulid>` tag follows such prose.

use txtodo_core::OwnedLine;
use txtodo_model::{TaskId, Ulid};

/// The `id:` tag's ULID without parsing the line, or `None` (no tag, invalid value, opaque line).
pub fn fast_id_of(line: &OwnedLine) -> Option<TaskId> {
    let raw = line.raw()?;
    let value = raw
        .split(|c: char| c.is_ascii_whitespace())
        .filter_map(|word| word.strip_prefix("id:"))
        .find(|value| !value.is_empty())?;
    let id = Ulid::parse(value).map(TaskId::new);
    debug_assert!(
        id.is_none() || value.len() == 26,
        "a parsed ULID is 26 chars"
    );
    id
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::id_of;
    use proptest::prelude::*;
    use txtodo_core::LineEnding;

    fn line(s: &str) -> OwnedLine {
        OwnedLine::from_bytes(s.as_bytes().to_vec(), LineEnding::default())
    }

    #[test]
    fn matches_the_parser_on_the_tricky_shapes() {
        let ok = "01ARZ3NDEKTSV4RRFFQ69G5FAV";
        for s in [
            format!("(A) 2026-09-11 buy id:{ok}"),
            format!("id:{ok}"),
            format!("x 2026-09-11 done id:{ok} pri:A"),
            format!("see http://x.io/id:{ok} id:{ok}"),
            format!("bad id:nope then id:{ok}"),
            format!("key:id:{ok}"),
            format!("tab\tid:{ok}\tafter"),
            // A bare `id:` (empty value, immediately followed by whitespace) is not a tag per
            // `specs/todotxt.abnf`'s `tag = key ":" 1*NONSP` — the parser skips it as plain text
            // and keeps looking, so the fast scan must too. Regression for this project's own
            // todo.txt, whose task descriptions mention "id: tag" by name.
            format!("stamps id: tag; --no-id flag; config id:{ok}"),
            format!("bare id: then id:{ok}"),
            "id:".to_owned(),
            String::new(),
            "   ".to_owned(),
        ] {
            assert_eq!(fast_id_of(&line(&s)), id_of(&line(&s)), "{s:?}");
        }
    }

    proptest! {
        #[test]
        fn agrees_with_the_parser(s in "[ a-z(A)x0-9:/.\\t-]{0,40}( id:[0-7][0-9A-HJKMNP-TV-Z]{25})?[ a-z:]{0,10}") {
            prop_assert_eq!(fast_id_of(&line(&s)), id_of(&line(&s)));
        }
    }
}
