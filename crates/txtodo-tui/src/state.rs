//! `AppState`: everything the UI widgets render from — lines (with completion/id derived by
//! [`txtodo_core::tokenize`], never a full parse, per design §7), the vim cursor, the edit draft,
//! the sync snapshot and the conflict-review flags. Built standalone against hand-built fixtures
//! here (recommended build order step 2); `app.rs` (step 4) is the only place that mutates it from
//! real `Daemon` calls.

use txtodo_core::{TokenKind, Ulid, tokenize};

use crate::state_offers::OffersPane;

/// One line of the document as the UI needs it: the raw bytes (never mutated in place — every
/// change is an `Apply`, design §7's invariant) plus the two facts widgets need without
/// re-tokenizing on every keystroke.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LineState {
    /// 1-based line number, blanks included — the same numbering `TaskRef.line_number` uses.
    pub line_number: u32,
    /// The exact raw text the daemon holds for this line (no line ending).
    pub raw: String,
    /// Whether the line's structured prefix starts with the completion marker `x`.
    pub completed: bool,
    /// The task's `id:` tag, parsed out by `tokenize`, if the line carries a valid one.
    pub task_id: Option<String>,
}

impl LineState {
    /// Classifies one raw line via `tokenize` only — never a full parse, per design §7 ("None of
    /// them parse the file themselves").
    pub fn from_raw(line_number: u32, raw: impl Into<String>) -> Self {
        let raw = raw.into();
        let mut completed = false;
        let mut task_id = None;
        for span in tokenize(&raw) {
            match span.kind {
                TokenKind::CompletionMarker => completed = true,
                TokenKind::IdTag => {
                    // `id:<ULID>`; tokenize only classifies this way when the ULID already
                    // parses (see `txtodo_core::tokenize::classify_word`), so this always
                    // succeeds, but stay defensive rather than unwrap.
                    // Ref: https://docs.rs/ulid (this workspace's own `Ulid::parse`)
                    let rest = raw[span.start..span.end].strip_prefix("id:");
                    let valid = rest.filter(|r| Ulid::parse(r).is_some());
                    task_id = valid.map(str::to_owned);
                }
                _ => {}
            }
        }
        LineState {
            line_number,
            raw,
            completed,
            task_id,
        }
    }

    /// The `TaskRef` shape (`Apply`/conflict RPCs) addresses a line by both its number and id;
    /// the daemon rejects the mutation when they disagree (design §7's stale-`TaskRef` rule).
    pub fn task_ref_id(&self) -> &str {
        self.task_id.as_deref().unwrap_or("")
    }
}

/// One peer in the sync indicator (`s`), the UI-local mirror of `pb::SyncStatusResponse::Peer`
/// (`app.rs::to_sync_snapshot` maps one to the other) — kept as its own type rather than using
/// the generated `pb` one directly so this module and `ui/sync.rs` stay daemon/proto-free and
/// fixture-testable, the same idiom `ConflictItem` already uses for `pb::ReviewFlag`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PeerStatus {
    /// Device id (ULID text).
    pub device: String,
    /// How far behind that peer's last-seen timestamp is, in milliseconds.
    pub lag_ms: i64,
}

/// The sync indicator's last known snapshot.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SyncSnapshot {
    /// Every paired peer this device currently knows about.
    pub peers: Vec<PeerStatus>,
    /// Ops applied locally that have not yet reached every peer.
    pub pending_ops: u32,
}

/// One flagged conflict for the `r` pane, the UI-local mirror of `pb::ReviewFlag`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConflictItem {
    /// Task id (ULID text).
    pub task_id: String,
    /// 1-based line number the task sits on now; `0` when it is no longer in the file.
    pub line_number: u32,
    /// This device's description at flag time.
    pub mine: String,
    /// The peer's description at flag time.
    pub theirs: String,
}

/// Which side of a conflict the human picked in the `r` pane.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Resolution {
    /// Keep this device's text.
    Mine,
    /// Keep the peer's text.
    Theirs,
    /// Keep whatever is in the file now; only clears the flag.
    Merged,
}

/// What the single-line editor (`i`/`a`/`A`) is currently composing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EditTarget {
    /// Editing the existing line at this 1-based line number.
    Existing {
        /// 1-based line number.
        line_number: u32,
        /// The line's task id, if it has one yet.
        task_id: Option<String>,
    },
    /// Composing the trailing "Add a line" row (design §3.1).
    NewLine,
}

/// The single-line editor's in-progress buffer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EditDraft {
    /// What this draft will become on save.
    pub target: EditTarget,
    /// The raw text being edited, including any `id:` tag (design §7: "always present in the
    /// edit field").
    pub buffer: String,
    /// Byte offset of the caret into `buffer`; always a char boundary.
    pub caret: usize,
}

impl EditDraft {
    /// Starts editing an existing line, caret at `at_start` ? 0 : end-of-line — `i`/`A` semantics
    /// (`a` is handled by the caller advancing the caret one grapheme after construction).
    pub fn for_line(line: &LineState, at_start: bool) -> Self {
        let buffer = line.raw.clone();
        let caret = if at_start { 0 } else { buffer.len() };
        EditDraft {
            target: EditTarget::Existing {
                line_number: line.line_number,
                task_id: line.task_id.clone(),
            },
            buffer,
            caret,
        }
    }

    /// Starts composing a brand-new line on the trailing "Add a line" row.
    pub fn new_line() -> Self {
        EditDraft {
            target: EditTarget::NewLine,
            buffer: String::new(),
            caret: 0,
        }
    }
}

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
    /// Whether `id:` tags are painted (header toggle, design §3.1).
    pub show_id: bool,
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
            show_id: false,
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

    /// `id:` visibility header toggle.
    pub fn toggle_show_id(&mut self) {
        self.show_id = !self.show_id;
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
