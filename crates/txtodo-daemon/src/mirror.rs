//! The actor's Loro mirror of one document (plan M4; tasks/crdt-loro-state as re-planned on
//! 2026-09-12). `DocState` stays the byte-faithful, Vec-backed truth for what is on disk; the
//! mirror is a `txtodo_crdt::LoroDocument` fed the same committed ops, so a merge engine with the
//! full local history exists for sync to export and import against. It is derived state: it is
//! flushed *after* a commit, never consulted for bytes, and rebuilt from the state whenever it
//! disagrees or the state was adopted from disk. Measured: a per-op mirror inside the state cost
//! 168 s per 10k-line adopt and 2.5 s per clone (fork); this shape costs one flush per commit.

use std::fmt;

use crate::state::{DocState, Entry};
use txtodo_core::LineKind;
use txtodo_crdt::{HydrateLine, LoroDocument, hydrate_file};
use txtodo_model::{DeviceId, FilePath, Hlc, Op, OpKind, TaskId, Ulid};

/// Why the mirror could not follow the state. Logged and healed by a rebuild; never a client error.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MirrorError {
    /// The crdt crate refused an op the state had accepted.
    Refused {
        /// Which op, by kind name.
        kind: &'static str,
        /// The crdt's message.
        message: String,
    },
    /// Hydration from the state failed.
    Hydrate(String),
}

impl fmt::Display for MirrorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MirrorError::Refused { kind, message } => {
                write!(f, "mirror refused {kind}: {message}")
            }
            MirrorError::Hydrate(m) => write!(f, "mirror hydration failed: {m}"),
        }
    }
}

impl std::error::Error for MirrorError {}

/// One document's Loro mirror.
pub struct Mirror {
    doc: LoroDocument,
    path: FilePath,
}

impl Mirror {
    /// A fresh mirror holding exactly the state's visible lines, hydrated under a zero stamp so
    /// any real op's field write wins over the hydrated value.
    pub fn from_state(state: &DocState) -> Result<Mirror, MirrorError> {
        let mut doc = LoroDocument::open();
        let entries: Vec<Entry> = (0..state.len()).filter_map(|i| state.entry_at(i)).collect();
        debug_assert_eq!(entries.len(), state.len());
        let lines: Vec<HydrateLine<'_>> = entries
            .iter()
            .map(|e| match e {
                Entry::Task { id, line } => HydrateLine::Task {
                    task: *id,
                    // from_file validated UTF-8; an opaque line never reaches the state.
                    line: line.raw().unwrap_or_default(),
                },
                Entry::Blank(_) => HydrateLine::Blank,
            })
            .collect();
        let zero = Hlc::zero(DeviceId::new(Ulid::from_u128(0)));
        hydrate_file(&mut doc, state.path(), &lines, zero)
            .map_err(|e| MirrorError::Hydrate(e.to_string()))?;
        let mirror = Mirror {
            doc,
            path: state.path().clone(),
        };
        debug_assert_eq!(mirror.doc.list_ids(&mirror.path).len(), state.len());
        Ok(mirror)
    }

    /// The document, for sync to export from.
    pub fn doc(&self) -> &LoroDocument {
        &self.doc
    }

    /// Applies committed `ops` in order and keeps each touched task's description text equal to
    /// the state's line (a prefix rewrite such as `complete` may append ` pri:B`). Stops at the
    /// first refusal; the caller rebuilds from the state.
    pub fn flush(&mut self, ops: &[Op], state: &DocState) -> Result<(), MirrorError> {
        debug_assert!(
            ops.iter().all(|o| o.file == self.path),
            "ops for this document"
        );
        // Bounded by the batch the actor committed (≤ MAX_MUTATIONS_PER_APPLY or one reconcile).
        for op in ops {
            txtodo_crdt::apply(&mut self.doc, op).map_err(|e| MirrorError::Refused {
                kind: kind_name(&op.kind),
                message: e.to_string(),
            })?;
            if let Some(task) = touched_task(&op.kind) {
                self.settle_description(task, state)?;
            }
        }
        debug_assert!(
            ops.is_empty() || self.agrees_with(state),
            "flush keeps the mirror in step"
        );
        Ok(())
    }

    /// True when the mirror's visible lines match the state: same ids in order, blanks where
    /// blanks are, and every description text equal to the line's. O(n); for asserts and tests.
    pub fn agrees_with(&self, state: &DocState) -> bool {
        let visible: Vec<TaskId> = self
            .doc
            .list_ids(&self.path)
            .into_iter()
            .filter(|id| txtodo_crdt::is_blank(*id) || !self.doc.is_deleted(*id))
            .collect();
        if visible.len() != state.len() {
            return false;
        }
        visible
            .iter()
            .enumerate()
            .all(|(i, id)| match state.entry_at(i) {
                Some(Entry::Blank(_)) => txtodo_crdt::is_blank(*id),
                Some(Entry::Task { id: want, line }) => {
                    *id == want && self.doc.description(*id) == description_of(&line)
                }
                None => false,
            })
    }

    /// Makes the Loro description of `task` equal to the state's line, if the task is live.
    fn settle_description(&mut self, task: TaskId, state: &DocState) -> Result<(), MirrorError> {
        let Some(line) = state.line_of(task) else {
            return Ok(());
        };
        let Some(want) = description_of(&line) else {
            return Ok(());
        };
        if self.doc.description(task).as_deref() == Some(want.as_str()) {
            return Ok(());
        }
        self.doc
            .set_description(task, &want)
            .map_err(|e| MirrorError::Refused {
                kind: "set_description",
                message: e.to_string(),
            })?;
        debug_assert_eq!(self.doc.description(task), Some(want));
        Ok(())
    }
}

fn description_of(line: &txtodo_core::OwnedLine) -> Option<String> {
    match line.parse()?.kind {
        LineKind::Task(t) => Some(t.description.to_owned()),
        LineKind::Blank => None,
    }
}

/// The task whose line an op may have rewritten, if any.
fn touched_task(kind: &OpKind) -> Option<TaskId> {
    match kind {
        OpKind::Insert { task, .. }
        | OpKind::SetField { task, .. }
        | OpKind::EditText { task, .. } => Some(*task),
        OpKind::Move { .. }
        | OpKind::BlankInsert { .. }
        | OpKind::BlankRemove { .. }
        | OpKind::NotesEdit { .. } => None,
    }
}

fn kind_name(kind: &OpKind) -> &'static str {
    match kind {
        OpKind::Insert { .. } => "Insert",
        OpKind::SetField { .. } => "SetField",
        OpKind::EditText { .. } => "EditText",
        OpKind::Move { .. } => "Move",
        OpKind::BlankInsert { .. } => "BlankInsert",
        OpKind::BlankRemove { .. } => "BlankRemove",
        OpKind::NotesEdit { .. } => "NotesEdit",
    }
}
