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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_slug_is_the_ref_tags_value() {
        assert_eq!(ref_slug("plan the trip ref:trip +home"), Some("trip"));
        assert_eq!(ref_slug("no ref here"), None);
        assert_eq!(ref_slug("empty ref: tag"), None);
    }
}
