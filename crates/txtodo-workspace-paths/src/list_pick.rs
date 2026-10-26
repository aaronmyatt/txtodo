//! Which named list a user means by a name (task `two-kinds-of-workspace`, ADR 0043): the one rule
//! the daemon, the CLI and MCP share, so `--list work` means the same list everywhere.
//!
//! A list answers to the name it shows (renamed or not), and failing that to the name it was made
//! under, which is its folder's name in the lists root. Case does not matter. Two lists that show
//! the same name (two devices renamed at once) are ambiguous; the folder name still reaches either.

use std::path::Path;

/// One list a name is matched against: the name it shows, and its folder.
#[derive(Clone, Copy, Debug)]
pub struct ListRef<'a> {
    /// The name the list shows (`WorkspaceInfo.name`).
    pub shown: &'a str,
    /// The list's folder; its last component is the name the list was made under.
    pub root: &'a Path,
}

/// What [`pick_list`] found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Pick {
    /// The list at this index.
    Found(usize),
    /// No list answers to the name.
    NotFound,
    /// Several lists show the name; these are their indexes.
    Ambiguous(Vec<usize>),
}

/// The list `query` means among `lists`: the one that shows that name, else the one made under it.
pub fn pick_list(lists: &[ListRef<'_>], query: &str) -> Pick {
    let wanted = query.trim().to_ascii_lowercase();
    let shown: Vec<usize> = (0..lists.len())
        .filter(|&i| lists[i].shown.eq_ignore_ascii_case(&wanted))
        .collect();
    match shown.as_slice() {
        [one] => return Pick::Found(*one),
        [] => {}
        _ => return Pick::Ambiguous(shown),
    }
    let made_under = |list: &ListRef<'_>| {
        list.root
            .file_name()
            .is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case(&wanted))
    };
    (0..lists.len())
        .find(|&i| made_under(&lists[i]))
        .map_or(Pick::NotFound, Pick::Found)
}

#[cfg(test)]
#[path = "list_pick_tests.rs"]
mod tests;
