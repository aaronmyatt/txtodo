//! Removes a task line's own `id:` tag for the Tagged → Sidecar migration
//! (`tasks/sidecar-migrate-tagged`). "Own" means exactly what `fast_id_of` decides: the first
//! whitespace-delimited `id:` word that has a value, provided that value is a ULID. A bare `id:`
//! in prose, an id quoted inside a description (`(id:01M2…)` is one word, and it does not start
//! with `id:`), and a later second tag are all left alone.

use crate::fastid::fast_id_of;
use txtodo_core::{Edit, OwnedLine, apply};
use txtodo_model::TaskId;

/// The line's own id and the line without that tag, or `None` when the line has no own tag (a
/// blank, an opaque line, an untagged task). The removal is `Edit::remove_tag`, which drops the
/// first `id:` word plus one adjacent space and touches nothing else, so the result is byte-for-byte
/// the original minus ` id:<ULID>`.
pub(crate) fn strip_own_id(line: &OwnedLine) -> Option<(TaskId, OwnedLine)> {
    let id = fast_id_of(line)?;
    // `remove_tag` only errors on an invalid key; "id" is valid.
    let edit = Edit::new().remove_tag("id").ok()?;
    let stripped = apply(line, &edit);
    Some((id, stripped))
}

/// [`strip_own_id`]'s bytes, without parsing the line where the cut is plain: the own `id:`
/// word with the space after it (a space or the line start before it), or with the space before
/// it at the end of a line whose word before is not a prefix token (`x`, `(A)`, a date). Anything
/// else (a tab beside it, a line that is all prefix) takes the parse: `Edit::remove_tag` works
/// inside the description, so the space after a prefix stays. `duplicates.rs` runs this over every
/// line on each commit with a Watch subscriber (task first-sync-speed: the parse was ~37% of a
/// sync commit); a property test below pins it to `strip_own_id`.
pub(crate) fn without_own_id(line: &OwnedLine) -> Option<Vec<u8>> {
    fast_id_of(line)?;
    let raw = line.raw()?;
    let bytes = raw.as_bytes();
    let (start, end) = own_id_word(raw)?;
    let before = start.checked_sub(1).map(|i| bytes[i]);
    let cut = match (before, bytes.get(end)) {
        (None | Some(b' '), Some(b' ')) => Some((start, end + 1)),
        (Some(b' '), None) if !after_prefix_token(&raw[..start - 1]) => Some((start - 1, end)),
        _ => None,
    };
    let Some((from, to)) = cut else {
        return strip_own_id(line).map(|(_, stripped)| stripped.bytes().to_vec());
    };
    let mut out = Vec::with_capacity(bytes.len());
    out.extend_from_slice(&bytes[..from]);
    out.extend_from_slice(&bytes[to..]);
    Some(out)
}

/// The byte span of the first whitespace-delimited `id:` word with a value: the word `fast_id_of`
/// reads. Every separator is one ASCII byte, so word starts add up.
fn own_id_word(raw: &str) -> Option<(usize, usize)> {
    let mut at = 0;
    for word in raw.split(|c: char| c.is_ascii_whitespace()) {
        if word.strip_prefix("id:").is_some_and(|v| !v.is_empty()) {
            return Some((at, at + word.len()));
        }
        at += word.len() + 1;
    }
    None
}

/// Whether `head`'s last word could be part of a todo.txt prefix (`x`, `(A)`, a date), or `head`
/// has none: then the next word may start the description.
fn after_prefix_token(head: &str) -> bool {
    let Some(last) = head.split(|c: char| c.is_ascii_whitespace()).next_back() else {
        return true;
    };
    let b = last.as_bytes();
    let priority = b.len() == 3 && b[0] == b'(' && b[1].is_ascii_uppercase() && b[2] == b')';
    let date = b.len() == 10 && b[4] == b'-' && b[7] == b'-';
    last.is_empty() || last == "x" || priority || date
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use txtodo_core::LineEnding;

    const ID: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";
    const OTHER: &str = "01M2B4ZWQDKT5HCCNP0ZA6VTK5";

    fn line(s: &str) -> OwnedLine {
        OwnedLine::from_bytes(s.as_bytes().to_vec(), LineEnding::default())
    }

    fn stripped(s: &str) -> Option<String> {
        strip_own_id(&line(s)).map(|(_, l)| l.raw().unwrap_or_default().to_owned())
    }

    #[test]
    fn drops_a_trailing_tag_and_its_space() {
        assert_eq!(
            stripped(&format!("2026-09-19 buy milk @home id:{ID}")).as_deref(),
            Some("2026-09-19 buy milk @home")
        );
    }

    #[test]
    fn drops_a_middle_tag_keeping_one_space() {
        assert_eq!(
            stripped(&format!("(A) buy id:{ID} milk")).as_deref(),
            Some("(A) buy milk")
        );
    }

    #[test]
    fn returns_the_id_it_removed() {
        let (id, _) = strip_own_id(&line(&format!("buy id:{ID}"))).unwrap();
        assert_eq!(id.ulid().to_string(), ID);
    }

    #[test]
    fn keeps_a_quoted_id_in_the_description() {
        let s = format!("todo_move is a stub (id:{OTHER}) — fix it id:{ID}");
        assert_eq!(
            stripped(&s).as_deref(),
            Some(format!("todo_move is a stub (id:{OTHER}) — fix it").as_str())
        );
    }

    #[test]
    fn keeps_prose_that_mentions_the_tag_by_name() {
        let s = format!("stamps id: tag; --no-id flag id:{ID}");
        assert_eq!(
            stripped(&s).as_deref(),
            Some("stamps id: tag; --no-id flag")
        );
    }

    #[test]
    fn leaves_a_completed_line_prefix_alone() {
        assert_eq!(
            stripped(&format!("x 2026-09-19 2026-09-18 done id:{ID} pri:A")).as_deref(),
            Some("x 2026-09-19 2026-09-18 done pri:A")
        );
    }

    #[test]
    fn none_without_an_own_tag() {
        for s in ["", "   ", "buy milk", "buy id:nope", "bare id: only", "id:"] {
            assert!(strip_own_id(&line(s)).is_none(), "{s:?}");
        }
    }

    proptest! {
        // The stripped line is the original with exactly one `id:<ULID>` word gone, and it no
        // longer starts with that id's own tag unless a second valid tag follows.
        #[test]
        fn removes_exactly_the_own_tag(
            head in "[a-z]{1,8}( [a-z@+]{1,8}){0,3}",
            tail in "( [a-z]{1,8}){0,3}",
        ) {
            let original = format!("{head} id:{ID}{tail}");
            let expected = format!("{head}{tail}");
            prop_assert_eq!(stripped(&original), Some(expected));
        }
    }

    fn token() -> impl Strategy<Value = String> {
        prop_oneof![
            "[a-z]{1,4}".prop_map(|w| w),
            Just(format!("id:{ID}")),
            Just(format!("id:{OTHER}")),
            Just("id:".to_owned()),
            Just("id:nope".to_owned()),
            Just(format!("(id:{OTHER})")),
            Just("(A)".to_owned()),
            Just("x 2026-09-19".to_owned()),
            Just("pri:A".to_owned()),
        ]
    }

    proptest! {
        // The byte cut `duplicates.rs` uses gives exactly `strip_own_id`'s bytes.
        #[test]
        fn without_own_id_matches_strip_own_id(
            words in proptest::collection::vec(token(), 0..6),
            seps in proptest::collection::vec(prop_oneof![Just(" "), Just("  "), Just("\t")], 6),
            lead in prop_oneof![Just(""), Just(" ")],
        ) {
            let mut s = lead.to_owned();
            for (w, sep) in words.iter().zip(&seps) {
                s.push_str(w);
                s.push_str(sep);
            }
            let l = line(s.trim_end_matches(['\t', ' ']));
            let want = strip_own_id(&l).map(|(_, stripped)| stripped.bytes().to_vec());
            prop_assert_eq!(without_own_id(&l), want);
        }
    }
}
