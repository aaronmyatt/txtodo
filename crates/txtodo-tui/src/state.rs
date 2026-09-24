//! `AppState`: everything the UI widgets render from — lines (with completion/id derived by
//! [`txtodo_core::tokenize`], never a full parse, per design §7), the vim cursor, the edit draft,
//! the sync snapshot and the conflict-review flags. Built standalone against hand-built fixtures
//! here (recommended build order step 2); `app.rs` (step 4) is the only place that mutates it from
//! real `Daemon` calls.

use crate::hit::HitMap;
use crate::state_offers::OffersPane;
use crate::state_shell::Shell;

pub use crate::state_nav::{Focus, Nav, Overlay, Screen, SettingsCard};
pub use crate::state_types::{
    ConflictItem, EditDraft, EditTarget, LineState, PeerStatus, Resolution, SyncSnapshot,
};

/// Everything the UI renders from. `cursor == lines.len()` means the trailing "Add a line" row is
/// selected (design §3.1: "the last line of the document is always an empty Add a line row").
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppState {
    /// The workspace-relative path currently open, e.g. `"todo.txt"`.
    pub path: String,
    /// Every line of the document, in file order.
    pub lines: Vec<LineState>,
    /// Index into `lines`; `lines.len()` selects the trailing Add-a-line row.
    pub cursor: usize,
    /// The single-line editor's draft, when `i`/`a`/`A` is active; `None` otherwise.
    pub editing: Option<EditDraft>,
    /// Whether the conflict-review pane (`r`) is open.
    pub conflicts_open: bool,
    /// Open `needs_review` flags for `path` (plan M4), surfaced via `Watch`.
    pub needs_review: Vec<ConflictItem>,
    /// Selected index into `needs_review` while the pane is open.
    pub conflict_cursor: usize,
    /// Whether the sync indicator (`s`) is shown.
    pub sync_visible: bool,
    /// The sync indicator's last known snapshot.
    pub sync: SyncSnapshot,
    /// The `:` command line's in-progress buffer (`None` when not in command mode). `:q` quits.
    pub command: Option<String>,
    /// Set once `:q` is entered; the event loop exits when this is true.
    pub should_quit: bool,
    /// Advisory-only (`crate::skill_hint::needed`), painted in the status line by `ui/screen.rs`.
    /// `from_document` defaults this to `false`; `app.rs::run` sets the real value once at
    /// startup, against the real `$HOME` — pure-rendering callers (fixtures, tests) never touch
    /// the filesystem.
    pub skill_hint: bool,
    /// Names the workspace in the status line when it is not the folder the user started in: `Some`
    /// for the default workspace (task default-workspace), `None` otherwise.
    pub workspace_label: Option<String>,
    /// The `o` workspace-offers pane (task `workspace-offer-cli`).
    pub offers: OffersPane,
    /// The daemon's last refusal of an action (a stale line, a blank line addressed), shown in
    /// the status line until the next action succeeds. A refusal used to end the whole session.
    pub last_error: Option<String>,
    /// The screen, focus and overlay (task `tui-revamp/tui-foundation`).
    pub nav: Nav,
    /// Set when `path` now lives in another workspace: the loop opens a new `Watch` for it.
    pub rewatch: bool,
    /// The list row at the top of the view; the wheel moves it, and drawing keeps the cursor in it.
    pub scroll: usize,
    /// The list row under the mouse pointer, painted as hovered.
    pub hover: Option<usize>,
    /// Where the last frame put each clickable thing (task `tui-revamp/tui-mouse`).
    pub hits: HitMap,
    /// The header and its popups (task `tui-revamp/tui-shell`).
    pub shell: Shell,
}

impl AppState {
    /// Builds state from a document's raw bytes, one `LineState` per line (blank lines included
    /// and numbered, design §3.1). `path` is workspace-relative, e.g. `"todo.txt"`.
    pub fn from_document(path: impl Into<String>, raw: &str) -> Self {
        let lines = raw
            .lines()
            .enumerate()
            .map(|(i, line)| LineState::from_raw(i as u32 + 1, line))
            .collect();
        AppState {
            path: path.into(),
            lines,
            cursor: 0,
            editing: None,
            conflicts_open: false,
            needs_review: Vec::new(),
            conflict_cursor: 0,
            sync_visible: false,
            sync: SyncSnapshot::default(),
            command: None,
            should_quit: false,
            skill_hint: false,
            workspace_label: None,
            offers: OffersPane::default(),
            last_error: None,
            nav: Nav::default(),
            rewatch: false,
            scroll: 0,
            hover: None,
            hits: HitMap::default(),
            shell: Shell::default(),
        }
    }

    /// A hand-built fixture: a small document with a priority, a project/context, a tag, a
    /// completed line, a blank line and an `id:` tag — enough for `ui/list.rs`/`ui/edit.rs` tests
    /// to exercise every §3.1 rendering rule without a live daemon (recommended build order step 2).
    pub fn fixture() -> Self {
        let raw = "(A) 2026-09-11 Call the plumber +house @phone due:2026-09-15\n\
                    x 2026-09-10 2026-09-01 Renew passport +admin id:01J9K3H5Z7Q8X2M4N6P8R0T2V4\n\
                    \n\
                    Water the plants +home @garden";
        let mut state = AppState::from_document("todo.txt", raw);
        state.sync = SyncSnapshot {
            peers: vec![PeerStatus {
                device: "01J9K3H5Z7Q8X2M4N6P8R0T2V5".to_owned(),
                lag_ms: 400,
            }],
            pending_ops: 2,
        };
        state.needs_review = vec![ConflictItem {
            task_id: "01J9K3H5Z7Q8X2M4N6P8R0T2V4".to_owned(),
            line_number: 2,
            mine: "Renew passport +admin".to_owned(),
            theirs: "Renew passport +admin +urgent".to_owned(),
        }];
        state
    }

    /// The number of selectable rows: every line, plus the trailing Add-a-line row.
    pub fn row_count(&self) -> usize {
        self.lines.len() + 1
    }

    /// The line the cursor is on; `None` when the cursor is on the trailing Add-a-line row.
    pub fn selected_line(&self) -> Option<&LineState> {
        self.lines.get(self.cursor)
    }

    /// Whether the cursor is on the trailing Add-a-line row.
    pub fn on_add_line_row(&self) -> bool {
        self.cursor == self.lines.len()
    }

    /// `j`: one row down, clamped at the Add-a-line row.
    pub fn move_down(&mut self) {
        self.cursor = (self.cursor + 1).min(self.lines.len());
    }

    /// `k`: one row up, clamped at the first line.
    pub fn move_up(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
    }

    /// `gg`: jump to the first line.
    pub fn move_first(&mut self) {
        self.cursor = 0;
    }

    /// `G`: jump to the last row (the Add-a-line row, since it is always the last row).
    pub fn move_last(&mut self) {
        self.cursor = self.lines.len();
    }

    /// `s`: toggles the sync indicator.
    pub fn toggle_sync_visible(&mut self) {
        self.sync_visible = !self.sync_visible;
    }

    /// `r`: toggles the conflict-review pane; resets its cursor on open.
    pub fn toggle_conflicts(&mut self) {
        self.conflicts_open = !self.conflicts_open;
        self.conflict_cursor = 0;
    }

    /// `:`: opens the command line.
    pub fn start_command(&mut self) {
        self.command = Some(String::new());
    }

    /// `Esc`: closes the command line without acting on it.
    pub fn cancel_command(&mut self) {
        self.command = None;
    }

    /// `Enter`: takes the command line's buffer and, if it is exactly `q`, quits (design: "`:q`
    /// quit" — no other `:` commands are specified, so anything else is a silent no-op rather
    /// than an invented feature).
    pub fn run_command(&mut self) {
        if self.command.take().as_deref() == Some("q") {
            self.should_quit = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_raw_classifies_completion_and_id() {
        let l = LineState::from_raw(1, "x 2026-09-10 done id:01J9K3H5Z7Q8X2M4N6P8R0T2V4");
        assert!(l.completed);
        assert_eq!(l.task_id.as_deref(), Some("01J9K3H5Z7Q8X2M4N6P8R0T2V4"));

        let l = LineState::from_raw(2, "not done +proj");
        assert!(!l.completed);
        assert_eq!(l.task_id, None);
    }

    #[test]
    fn from_raw_ignores_a_bad_ulid() {
        let l = LineState::from_raw(1, "task id:not-a-ulid");
        assert_eq!(
            l.task_id, None,
            "an invalid id: is an ordinary tag, not IdTag"
        );
    }

    #[test]
    fn blank_lines_are_kept_and_numbered() {
        let state = AppState::from_document("todo.txt", "a\n\nb");
        assert_eq!(state.lines.len(), 3);
        assert_eq!(state.lines[1].raw, "");
        assert_eq!(state.lines[1].line_number, 2);
    }

    #[test]
    fn navigation_clamps_at_the_add_line_row() {
        let mut state = AppState::fixture();
        assert_eq!(state.row_count(), state.lines.len() + 1);
        state.move_last();
        assert!(state.on_add_line_row());
        state.move_down();
        assert!(
            state.on_add_line_row(),
            "moving down past the end stays clamped"
        );
        state.move_first();
        assert_eq!(state.cursor, 0);
        state.move_up();
        assert_eq!(state.cursor, 0, "moving up past the start stays clamped");
    }

    #[test]
    fn fixture_seeds_sync_and_conflicts() {
        let state = AppState::fixture();
        assert_eq!(state.sync.peers.len(), 1);
        assert_eq!(state.sync.pending_ops, 2);
        assert_eq!(state.needs_review.len(), 1);
    }

    #[test]
    fn edit_draft_for_line_places_caret() {
        let line = LineState::from_raw(1, "buy milk");
        let start = EditDraft::for_line(&line, true);
        assert_eq!(start.caret, 0);
        let end = EditDraft::for_line(&line, false);
        assert_eq!(end.caret, "buy milk".len());
    }
}
