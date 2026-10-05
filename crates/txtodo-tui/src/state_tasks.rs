//! The Tasks screen's own state (task `tui-revamp/tui-tasks`): the `ref:` badges its rows show,
//! read from the daemon's file tree. Kept out of `state.rs` for its line budget.

use std::collections::BTreeMap;

/// What a line's `ref:` directory holds, as its row shows it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RefBadge {
    /// Its `todo.txt`: done of total task lines.
    Progress {
        /// Completed lines.
        done: u32,
        /// Task lines, blanks excluded.
        total: u32,
    },
    /// Only a `notes.md`.
    Notes,
}

/// The Tasks screen's state.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TasksView {
    /// Badges by `ref:` slug, for the open document's lines.
    pub refs: BTreeMap<String, RefBadge>,
}

/// The `ref:` slug a line names, if any.
pub fn ref_slug(raw: &str) -> Option<&str> {
    raw.split_whitespace()
        .find_map(|word| word.strip_prefix("ref:"))
        .filter(|slug| !slug.is_empty())
}

/// `raw` without its `ref:` tag, as the detail panel shows and edits a parent line: the tag is
/// plumbing (the panel already is that folder), and deleting it by hand would cut the link.
/// Ref: core `Edit::remove_tag`.
pub fn without_ref(raw: &str) -> String {
    let line = txtodo_core::OwnedLine::from_bytes(raw.as_bytes().to_vec(), Default::default());
    match txtodo_core::Edit::new().remove_tag("ref") {
        Ok(edit) => txtodo_core::apply(&line, &edit)
            .raw()
            .unwrap_or(raw)
            .to_owned(),
        Err(_) => raw.to_owned(),
    }
}

/// `edited` with `slug` put back as its `ref:` tag, unless the user typed a `ref:` of their own.
pub fn with_ref(edited: &str, slug: Option<&str>) -> String {
    let Some(slug) = slug.filter(|_| ref_slug(edited).is_none()) else {
        return edited.to_owned();
    };
    let line = txtodo_core::OwnedLine::from_bytes(edited.as_bytes().to_vec(), Default::default());
    match txtodo_core::Edit::new().set_tag("ref", slug) {
        Ok(edit) => txtodo_core::apply(&line, &edit)
            .raw()
            .unwrap_or(edited)
            .to_owned(),
        Err(_) => edited.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_ref_tag_is_hidden_for_editing_and_put_back_on_save() {
        let raw = "plan the trip ref:trip id:01ARZ3NDEKTSV4RRFFQ69G5FAA";
        let shown = without_ref(raw);
        assert_eq!(shown, "plan the trip id:01ARZ3NDEKTSV4RRFFQ69G5FAA");
        let saved = with_ref(
            "plan the big trip id:01ARZ3NDEKTSV4RRFFQ69G5FAA",
            ref_slug(raw),
        );
        assert_eq!(ref_slug(&saved), Some("trip"), "{saved}");
        assert!(saved.starts_with("plan the big trip"), "{saved}");
        assert_eq!(
            with_ref("go ref:mine", Some("trip")),
            "go ref:mine",
            "theirs wins"
        );
        assert_eq!(with_ref("go", None), "go");
    }

    #[test]
    fn the_slug_is_the_ref_tags_value() {
        assert_eq!(ref_slug("plan the trip ref:trip +home"), Some("trip"));
        assert_eq!(ref_slug("no ref here"), None);
        assert_eq!(ref_slug("empty ref: tag"), None);
    }
}
