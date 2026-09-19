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
}
