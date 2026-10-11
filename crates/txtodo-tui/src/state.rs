//! `AppState`: everything the UI widgets render from — lines (with completion/id derived by
//! [`txtodo_core::tokenize`], never a full parse, per design §7), the vim cursor, the edit draft,
//! the sync snapshot and the conflict-review flags. Built standalone against hand-built fixtures
//! here (recommended build order step 2); `app.rs` (step 4) is the only place that mutates it from
//! real `Daemon` calls.

use crate::hit::HitMap;
use crate::state_detail::Detail;
use crate::state_offers::OffersPane;
use crate::state_settings::SettingsView;
use crate::state_shell::Shell;
use crate::state_tasks::TasksView;
use crate::state_universal::UniversalView;

pub use crate::state_nav::{Focus, Nav, Overlay, Screen, SettingsCard};
pub use crate::state_types::{
    DuplicateCopy, DuplicateGroup, EditDraft, EditTarget, LineState, PeerStatus, SyncSnapshot,
};

/// Everything the UI renders from. `cursor == lines.len()` means the "Add a line" row is selected.
/// That row is drawn first, above the lines (task tui-desktop-fixes-sync); `row_of` and
/// `index_at_row` map between an index and the row it is drawn on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppState {
    /// The workspace-relative path currently open, e.g. `"todo.txt"`.
    pub path: String,
    /// Every line of the document, in file order.
    pub lines: Vec<LineState>,
    /// Index into `lines`; `lines.len()` selects the Add-a-line row (drawn at the top).
    pub cursor: usize,
    /// The single-line editor's draft, when `i`/`a`/`A` is active; `None` otherwise.
    pub editing: Option<EditDraft>,
    /// Whether the conflict-review pane (`r`) is open.
    pub conflicts_open: bool,
    /// The open file's duplicate groups (ADR 0032), from `ListConflicts`.
    pub duplicates: Vec<DuplicateGroup>,
    /// Selected group while the sheet is open.
    pub conflict_cursor: usize,
    /// Whether the sync indicator (`s`) is shown.
    pub sync_visible: bool,
    /// The sync indicator's last known snapshot.
    pub sync: SyncSnapshot,
    /// The `:` command line's in-progress buffer (`None` when not in command mode). `:q` quits.
    pub command: Option<String>,
    /// The `:` line's caret.
    pub command_caret: crate::text_input::TailCaret,
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
    /// The Tasks screen's `ref:` badges (task `tui-revamp/tui-tasks`).
    pub tasks: TasksView,
    /// The detail panel (task `tui-revamp/tui-detail`).
    pub detail: Detail,
    /// The Universal screen (task `tui-revamp/tui-universal`).
    pub universal: UniversalView,
    /// The Settings screen (task `tui-revamp/tui-settings`).
    pub settings: SettingsView,
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
            duplicates: Vec::new(),
            conflict_cursor: 0,
            sync_visible: false,
            sync: SyncSnapshot::default(),
            command: None,
            command_caret: crate::text_input::TailCaret::default(),
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
            tasks: TasksView::default(),
            detail: Detail::default(),
            universal: UniversalView::default(),
            settings: SettingsView::default(),
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
                ..PeerStatus::default()
            }],
            pending_ops: 2,
            protocol: 2,
        };
        state
    }

    /// The number of selectable rows: every line, plus the Add-a-line row.
    pub fn row_count(&self) -> usize {
        self.lines.len() + 1
    }

    /// The row index `i` is drawn on: the Add-a-line row first, then the lines in file order.
    pub fn row_of(&self, i: usize) -> usize {
        if i >= self.lines.len() { 0 } else { i + 1 }
    }

    /// The index drawn on row `row` (the inverse of [`AppState::row_of`]).
    pub fn index_at_row(&self, row: usize) -> usize {
        match row {
            0 => self.lines.len(),
            r => (r - 1).min(self.lines.len()),
        }
    }

    /// The line the cursor is on; `None` when the cursor is on the trailing Add-a-line row.
    pub fn selected_line(&self) -> Option<&LineState> {
        self.lines.get(self.cursor)
    }

    /// Whether the cursor is on the trailing Add-a-line row.
    pub fn on_add_line_row(&self) -> bool {
        self.cursor == self.lines.len()
    }

    /// `j`: one row down, clamped at the last line.
    pub fn move_down(&mut self) {
        let row = (self.row_of(self.cursor) + 1).min(self.row_count() - 1);
        self.cursor = self.index_at_row(row);
    }

    /// `k`: one row up, clamped at the Add-a-line row.
    pub fn move_up(&mut self) {
        self.cursor = self.index_at_row(self.row_of(self.cursor).saturating_sub(1));
    }

    /// `gg`: jump to the top row, the Add-a-line row.
    pub fn move_first(&mut self) {
        self.cursor = self.lines.len();
    }

    /// `G`: jump to the bottom row, the last line (the Add-a-line row when there are none).
    pub fn move_last(&mut self) {
        self.cursor = self.index_at_row(self.row_count() - 1);
    }

    /// `s`: toggles the sync indicator.
    pub fn toggle_sync_visible(&mut self) {
        self.sync_visible = !self.sync_visible;
    }

    /// Items the review sheet walks: the duplicate groups.
    pub fn review_len(&self) -> usize {
        self.duplicates.len()
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
    fn navigation_runs_from_the_add_line_row_on_top_to_the_last_line() {
        let mut state = AppState::fixture();
        assert_eq!(state.row_count(), state.lines.len() + 1);
        state.move_first();
        assert!(state.on_add_line_row(), "the top row");
        state.move_up();
        assert!(
            state.on_add_line_row(),
            "moving up past the top stays clamped"
        );
        state.move_down();
        assert_eq!(state.cursor, 0, "then the first line");
        state.move_last();
        assert_eq!(state.cursor, state.lines.len() - 1);
        state.move_down();
        assert_eq!(
            state.cursor,
            state.lines.len() - 1,
            "clamped at the last line"
        );
        assert_eq!((state.row_of(state.lines.len()), state.row_of(0)), (0, 1));
        assert_eq!(
            (state.index_at_row(0), state.index_at_row(1)),
            (state.lines.len(), 0)
        );
    }

    #[test]
    fn fixture_seeds_sync() {
        let state = AppState::fixture();
        assert_eq!(state.sync.peers.len(), 1);
        assert_eq!(state.sync.pending_ops, 2);
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
