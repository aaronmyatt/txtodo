//! The window chrome's state (task `tui-revamp/tui-shell`): which workspace the header names, the
//! header's search text, the `W` workspace popup, the banners' inputs and the toasts. Kept out of
//! `state.rs` for its line budget.

use std::time::{Duration, Instant};

/// Everything the header and its popups draw from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Shell {
    /// The open workspace's root, canonical, as `WorkspaceInfo.root` spells it.
    pub root: String,
    /// The header search field's text (the Tasks search, task `tui-revamp/tui-tasks`).
    pub search: String,
    /// This session's recent queries, newest first (the suggestions panel).
    pub recent_searches: Vec<String>,
    /// The prompt bar's draft (task `tui-revamp/tui-prompt`); kept while it loses the keyboard.
    pub prompt: crate::state::EditDraft,
    /// When the prompt bar was last typed in: its strict hint waits for a pause.
    pub prompt_typed_at: Option<Instant>,
    /// The `W` popup's rows, filled when it opens.
    pub menu: WorkspaceMenu,
    /// Whether the daemon's `Watch` stream is up.
    pub link: Link,
    /// The daemon's version and release date (`Health`), when they are not this build's.
    pub daemon_build: Option<(String, String)>,
    /// The conflict banner is hidden until the next flag arrives.
    pub conflict_banner_hidden: bool,
    /// The last edit the daemon refused, kept so it can be copied back out.
    pub refused: Option<Refused>,
    /// When the last edit landed: the footer says "saved" for a moment.
    pub saved_at: Option<Instant>,
    /// Toasts, oldest first.
    pub toasts: Vec<Toast>,
    /// What the next Apply toasts when it lands; dropped when it is refused.
    pub pending_toast: Option<String>,
    /// This session's changes, newest last: what `u` and a toast's Undo take back.
    pub changes: Vec<Change>,
    /// The id the next change gets.
    pub next_change: u64,
    /// How far the Help screen is scrolled, in lines.
    pub help_scroll: u16,
}

/// One change this session made, as the daemon's `Undo` takes it back.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Change {
    /// Its id, which a toast names.
    pub id: u64,
    /// The document it changed.
    pub path: String,
    /// The ops it appended (`ApplyResponse.applied`; a delete that leaves a blank line is two):
    /// the daemon undoes op by op.
    pub ops: u32,
    /// The workspace it was made in, when not the open one (the Universal screen's `x`).
    pub workspace: Option<String>,
}

/// The most changes `u` remembers.
const CHANGES_KEPT: usize = 100;

/// How long a toast stays (the c2 mockup's 5 s).
pub const TOAST_FOR: Duration = Duration::from_secs(5);

/// A short note in the bottom-right corner, with Undo when it reports a change.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Toast {
    /// What happened.
    pub message: String,
    /// The [`Change`] its Undo takes back, while that change is still the newest.
    pub undo: Option<u64>,
    /// When it appeared.
    pub at: Instant,
}

impl Shell {
    /// Adds a toast at `now`.
    pub fn toast(&mut self, message: impl Into<String>, undo: Option<u64>, now: Instant) {
        self.toasts.push(Toast {
            message: message.into(),
            undo,
            at: now,
        });
    }

    /// Drops the toasts older than [`TOAST_FOR`].
    pub fn prune(&mut self, now: Instant) {
        self.toasts.retain(|t| now.duration_since(t.at) < TOAST_FOR);
    }

    /// The newest toast, when its change is still the newest one: the daemon undoes the newest
    /// ops, so any other toast's Undo would take back the wrong change.
    pub fn undoable(&self) -> Option<&Toast> {
        let newest = self.changes.last()?.id;
        self.toasts.last().filter(|t| t.undo == Some(newest))
    }

    /// Records a change that landed in the open workspace; returns its id.
    pub fn record(&mut self, path: &str, ops: u32) -> u64 {
        self.record_in(path, ops, None)
    }

    /// Records a change that landed in `workspace` (by id; `None` for the open one).
    pub fn record_in(&mut self, path: &str, ops: u32, workspace: Option<String>) -> u64 {
        let id = self.next_change;
        self.next_change += 1;
        self.changes.push(Change {
            id,
            path: path.to_owned(),
            ops,
            workspace,
        });
        if self.changes.len() > CHANGES_KEPT {
            self.changes.remove(0);
        }
        id
    }
}

impl Default for Shell {
    fn default() -> Self {
        Shell {
            root: String::new(),
            search: String::new(),
            recent_searches: Vec::new(),
            prompt: crate::state::EditDraft::new_line(),
            prompt_typed_at: None,
            menu: WorkspaceMenu::default(),
            link: Link::default(),
            daemon_build: None,
            conflict_banner_hidden: false,
            refused: None,
            saved_at: None,
            toasts: Vec::new(),
            pending_toast: None,
            changes: Vec::new(),
            next_change: 0,
            help_scroll: 0,
        }
    }
}

/// The `Watch` stream's state, for the daemon banner.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Link {
    /// Streaming.
    #[default]
    Up,
    /// Dropped; the 1 s tick is reconnecting.
    Connecting,
    /// Reconnecting ran past its bound; Retry starts again.
    Down,
}

/// An edit the daemon did not save.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Refused {
    /// The daemon's reason.
    pub error: String,
    /// The line as typed.
    pub text: String,
}

/// One workspace in the `W` popup.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MenuItem {
    /// `WorkspaceInfo.workspace_id`: what a switch names.
    pub id: String,
    /// The folder name, or `default` for the default workspace.
    pub name: String,
    /// Open tasks in it (`UniversalTasks`, done left out); `None` while the daemon has not loaded
    /// it, since only a loaded workspace is counted.
    pub open: Option<usize>,
    /// Its folder is gone (`root_exists = false`).
    pub missing: bool,
    /// It is the open one.
    pub current: bool,
}

/// The `W` popup: the workspaces, then a last "Manage workspaces" row at `items.len()`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WorkspaceMenu {
    /// The workspaces, in the daemon's order.
    pub items: Vec<MenuItem>,
    /// The selected row; `items.len()` is Manage workspaces.
    pub cursor: usize,
}

impl WorkspaceMenu {
    /// Fills the popup, the cursor on the open workspace.
    pub fn fill(&mut self, items: Vec<MenuItem>) {
        self.cursor = items.iter().position(|i| i.current).unwrap_or(0);
        self.items = items;
    }

    /// Next row, stopping at Manage workspaces.
    pub fn move_down(&mut self) {
        self.cursor = (self.cursor + 1).min(self.items.len());
    }

    /// Previous row.
    pub fn move_up(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
    }

    /// The selected workspace, or `None` on Manage workspaces.
    pub fn selected(&self) -> Option<&MenuItem> {
        self.items.get(self.cursor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_popup_opens_on_the_current_workspace_and_ends_on_manage() {
        let item = |id: &str, current| MenuItem {
            id: id.to_owned(),
            current,
            ..MenuItem::default()
        };
        let mut menu = WorkspaceMenu::default();
        menu.fill(vec![item("a", false), item("b", true)]);
        assert_eq!(menu.selected().map(|i| i.id.as_str()), Some("b"));
        menu.move_down();
        assert_eq!(menu.selected(), None, "Manage workspaces");
        menu.move_down();
        assert_eq!(menu.cursor, 2, "stops there");
        menu.move_up();
        menu.move_up();
        menu.move_up();
        assert_eq!(menu.cursor, 0);
    }
}
