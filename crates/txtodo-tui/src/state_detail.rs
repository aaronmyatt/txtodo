//! The detail panel's state (task `tui-revamp/tui-detail`): a stack of levels, like desktop's
//! `MainView` detail stack. Each level is one line opened: where that line lives, its `ref:`
//! directory, the sub-list in it and the `notes.md` beside it. The sub-list is a [`Doc`] shaped
//! like the root list's fields on `AppState`, so list mode runs on it unchanged: the input layer
//! swaps it in, runs the command, and swaps it back ([`swap_in`]).

use std::time::Instant;

use crate::state::{AppState, EditDraft, LineState};

/// Which part of the panel has the keyboard; Tab moves on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Part {
    /// The parent line's field.
    Parent,
    /// The sub-list, in list mode.
    #[default]
    Sub,
    /// The `notes.md` editor.
    Notes,
}

impl Part {
    /// The next part (Tab), wrapping.
    pub fn next(self) -> Part {
        match self {
            Part::Parent => Part::Sub,
            Part::Sub => Part::Notes,
            Part::Notes => Part::Parent,
        }
    }

    /// The previous part (Shift-Tab), wrapping.
    pub fn prev(self) -> Part {
        self.next().next()
    }
}

/// The line a level opened.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Parent {
    /// The document it lives in.
    pub path: String,
    /// Its 1-based line number there.
    pub line_number: u32,
    /// Its task id (from `RefDir`, authoritative even without an `id:` tag).
    pub task_id: String,
    /// Its text.
    pub raw: String,
    /// Whether it is done.
    pub completed: bool,
}

/// A list the panel shows: the same four fields `AppState` keeps for the root list.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Doc {
    /// Workspace-relative path, `<ref dir>/todo.txt`.
    pub path: String,
    /// Its lines; empty while the file does not exist.
    pub lines: Vec<LineState>,
    /// The selected row; `lines.len()` is the Add-a-line row.
    pub cursor: usize,
    /// The row at the top of the view.
    pub scroll: usize,
}

/// `notes.md` as it is being edited.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Notes {
    /// The text in the editor.
    pub text: String,
    /// The caret, a byte offset on a char boundary.
    pub caret: usize,
    /// What the daemon last had; `text != saved` means unsaved.
    pub saved: String,
    /// When the last key was typed: autosave waits for a pause.
    pub typed_at: Option<Instant>,
}

impl Notes {
    /// Whether the editor holds text the daemon does not have yet.
    pub fn dirty(&self) -> bool {
        self.text != self.saved
    }
}

/// One level of the stack.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Level {
    /// The line this level opened.
    pub parent: Parent,
    /// Its `ref:` directory (workspace-relative), whether or not it exists yet.
    pub dir: String,
    /// Whether the directory exists.
    pub dir_exists: bool,
    /// The sub-list.
    pub doc: Doc,
    /// The notes editor.
    pub notes: Notes,
    /// The parent line's draft while it is being edited.
    pub parent_draft: Option<EditDraft>,
}

/// The panel: open while it has a level.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Detail {
    /// The levels, the one shown last.
    pub levels: Vec<Level>,
    /// The part with the keyboard.
    pub part: Part,
}

impl Detail {
    /// Whether the panel is open.
    pub fn is_open(&self) -> bool {
        !self.levels.is_empty()
    }

    /// The level shown.
    pub fn top(&self) -> Option<&Level> {
        self.levels.last()
    }

    /// The level shown, mutably.
    pub fn top_mut(&mut self) -> Option<&mut Level> {
        self.levels.last_mut()
    }

    /// The breadcrumb: each level's parent line, cut to [`CRUMB_MAX`] chars.
    pub fn crumbs(&self) -> Vec<String> {
        self.levels.iter().map(|l| crumb(&l.parent.raw)).collect()
    }
}

/// The longest breadcrumb label (the c2 mockup's 28).
pub const CRUMB_MAX: usize = 28;

/// A line as a breadcrumb label: its words, cut to [`CRUMB_MAX`] chars with an ellipsis.
pub fn crumb(raw: &str) -> String {
    let text = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    if text.chars().count() <= CRUMB_MAX {
        return text;
    }
    let cut: String = text.chars().take(CRUMB_MAX - 1).collect();
    format!("{cut}\u{2026}")
}

/// Swaps `doc` with the root list's fields on `state`: call it, run a list command, call it
/// again. List mode, the line editor and mouse drags then work on the sub-list as they do on the
/// root list, and every `Apply` they build names the sub-list's path.
pub fn swap_in(state: &mut AppState, doc: &mut Doc) {
    std::mem::swap(&mut state.path, &mut doc.path);
    std::mem::swap(&mut state.lines, &mut doc.lines);
    std::mem::swap(&mut state.cursor, &mut doc.cursor);
    std::mem::swap(&mut state.scroll, &mut doc.scroll);
}

/// Runs `f` with the shown level's sub-list swapped in as the list; `None` with no level.
pub fn with_sub_list<T>(state: &mut AppState, f: impl FnOnce(&mut AppState) -> T) -> Option<T> {
    let mut doc = std::mem::take(&mut state.detail.top_mut()?.doc);
    swap_in(state, &mut doc);
    // The review flags belong to the root list; the sub-list's line numbers must not match them.
    let flags = std::mem::take(&mut state.needs_review);
    let out = f(state);
    state.needs_review = flags;
    swap_in(state, &mut doc);
    if let Some(level) = state.detail.top_mut() {
        level.doc = doc;
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crumbs_are_cut_to_28_chars() {
        assert_eq!(crumb("plan  the trip"), "plan the trip");
        let long = crumb(&"word ".repeat(20));
        assert_eq!(long.chars().count(), CRUMB_MAX);
        assert!(long.ends_with('\u{2026}'));
    }

    #[test]
    fn parts_cycle_both_ways() {
        assert_eq!(Part::Sub.next(), Part::Notes);
        assert_eq!(Part::Notes.next(), Part::Parent);
        assert_eq!(Part::Parent.prev(), Part::Notes);
        assert_eq!(Part::Sub.prev(), Part::Parent);
    }

    #[test]
    fn a_command_on_the_sub_list_runs_on_its_lines_and_leaves_the_root_alone() {
        let mut state = AppState::fixture();
        let root = (state.path.clone(), state.lines.len(), state.cursor);
        state.detail.levels.push(Level {
            doc: Doc {
                path: "tasks/trip/todo.txt".to_owned(),
                lines: vec![LineState::from_raw(1, "pack"), LineState::from_raw(2, "go")],
                ..Doc::default()
            },
            ..Level::default()
        });
        let path = with_sub_list(&mut state, |s| {
            s.move_down();
            s.path.clone()
        });
        assert_eq!(path.as_deref(), Some("tasks/trip/todo.txt"));
        assert_eq!((state.path.clone(), state.lines.len(), state.cursor), root);
        assert_eq!(state.detail.top().map(|l| l.doc.cursor), Some(1));
    }
}
