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

use crate::state::{DocState, StateError};
use txtodo_core::{File, OwnedLine};
use txtodo_model::{FilePath, IdentityMode, TaskId, Ulid};

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

impl DocState {
    /// Tagged-mode convenience: reads every id via `fast_id_of`, then `from_file`.
    pub fn from_tagged_file(path: FilePath, file: &File) -> Result<DocState, StateError> {
        let ids: Vec<Option<TaskId>> = file.lines.iter().map(fast_id_of).collect();
        Self::from_file(path, file, &ids, IdentityMode::Tagged)
    }
}

/// A synthetic op under a zero stamp (device 0, HLC 0): for the mirror's hydration and for tests,
/// so any real op's field write wins over it. Never stored, never sent.
#[cfg(test)]
pub(crate) fn hydration_op(path: &FilePath, kind: txtodo_model::OpKind) -> txtodo_model::Op {
    let zero = txtodo_model::DeviceId::new(Ulid::from_u128(0));
    let op = txtodo_model::Op {
        id: txtodo_model::OpId::new(Ulid::from_u128(0)),
        hlc: txtodo_model::Hlc::zero(zero),
        principal: txtodo_model::Principal::External { device: zero },
        file: path.clone(),
        kind,
    };
    debug_assert_eq!(op.hlc.wall_ms, 0);
    debug_assert_eq!(&op.file, path);
    op
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
