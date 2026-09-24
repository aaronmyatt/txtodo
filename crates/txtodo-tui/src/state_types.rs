//! The line, draft, sync and conflict types `AppState` is built from. Split out of `state.rs`
//! for its line budget (task `tui-revamp/tui-foundation`); `state.rs` re-exports every one, so
//! `crate::state::LineState` and friends keep resolving.

use txtodo_core::{TokenKind, Ulid, tokenize};

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
