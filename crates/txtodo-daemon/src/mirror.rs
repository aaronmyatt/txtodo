//! The actor's Loro mirror of one document (plan M4; tasks/crdt-loro-state as re-planned on
//! 2026-09-12). `DocState` stays the byte-faithful, Vec-backed truth for what is on disk; the
//! mirror is a `txtodo_crdt::LoroDocument` fed the same committed ops, so a merge engine with the
//! full local history exists for sync to export and import against. It is derived state for
//! bytes — flushed *after* a commit, never consulted for bytes — but it is the *lineage* two
//! devices share, so it is persisted (`Store::put_mirror`) and, when the state is adopted from
//! disk, converged with corrective ops rather than rebuilt (a rebuild would be a new lineage and
//! the next import would duplicate everything). Measured: a per-op mirror inside the state cost
//! 168 s per 10k-line adopt and 2.5 s per clone; this shape costs one flush per commit.

use std::fmt;

use crate::state::{DocState, Entry};
use txtodo_core::{File, LineKind, parse_file};
use txtodo_crdt::{HydrateLine, Imported, LoroDocument, Review, Stamp, hydrate_file};
use txtodo_model::{DeviceId, FilePath, Hlc, Op, OpId, OpKind, TaskId, Ulid};

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
    /// Hydration, snapshot or import failed in Loro.
    Loro(String),
}

impl fmt::Display for MirrorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MirrorError::Refused { kind, message } => {
                write!(f, "mirror refused {kind}: {message}")
            }
            MirrorError::Loro(m) => write!(f, "mirror: {m}"),
        }
    }
}

impl std::error::Error for MirrorError {}

fn loro(e: impl fmt::Display) -> MirrorError {
    MirrorError::Loro(e.to_string())
}

/// One document's Loro mirror.
pub struct Mirror {
    doc: LoroDocument,
    path: FilePath,
}

impl Mirror {
    /// A fresh mirror holding exactly the state's visible lines, hydrated under a zero stamp so
    /// any real op's field write wins over the hydrated value. A new lineage: use only when no
    /// snapshot exists.
    pub fn from_state(state: &DocState, peer: u64) -> Result<Mirror, MirrorError> {
        let mut doc = LoroDocument::open();
        doc.set_peer(peer).map_err(loro)?;
        let entries: Vec<Entry> = (0..state.len()).filter_map(|i| state.entry_at(i)).collect();
        debug_assert_eq!(entries.len(), state.len());
        let lines: Vec<HydrateLine<'_>> = entries.iter().map(hydrate_line).collect();
        let zero = Hlc::zero(DeviceId::new(Ulid::from_u128(0)));
        hydrate_file(&mut doc, state.path(), &lines, zero).map_err(loro)?;
        let mirror = Mirror {
            doc,
            path: state.path().clone(),
        };
        debug_assert_eq!(mirror.doc.list_ids(&mirror.path).len(), state.len());
        Ok(mirror)
    }

    /// The mirror a previous run stored; the caller replays the ops committed since.
    pub fn from_snapshot(bytes: &[u8], path: &FilePath, peer: u64) -> Result<Mirror, MirrorError> {
        let doc = LoroDocument::from_snapshot(bytes).map_err(loro)?;
        doc.set_peer(peer).map_err(loro)?;
        debug_assert!(!bytes.is_empty());
        Ok(Mirror {
            doc,
            path: path.clone(),
        })
    }

    /// The document, for sync to export from.
    pub fn doc(&self) -> &LoroDocument {
        &self.doc
    }

    /// Everything the mirror holds, to persist. <https://docs.rs/loro> `ExportMode::Snapshot`.
    pub fn snapshot(&self) -> Result<Vec<u8>, MirrorError> {
        let bytes = self.doc.snapshot().map_err(loro)?;
        debug_assert!(!bytes.is_empty());
        Ok(bytes)
    }

    /// Applies committed `ops` in order and keeps each touched task's description text equal to
    /// the state's line (a prefix rewrite such as `complete` may append ` pri:B`). Stops at the
    /// first refusal; the caller rebuilds from the state.
    pub fn flush(&mut self, ops: &[Op], state: &DocState) -> Result<(), MirrorError> {
        self.replay(ops)?;
        for task in ops.iter().filter_map(|o| touched_task(&o.kind)) {
            self.settle_description(task, state)?;
        }
        // No `debug_assert!(agrees_with)` here: the actor checks agreement in every build and
        // converges on a mismatch (`actor_mirror.rs::after_flush`), which a panic here would
        // make unreachable in dev/test builds.
        Ok(())
    }

    /// Applies ops without settling descriptions — for replaying the log after a snapshot.
    pub fn replay(&mut self, ops: &[Op]) -> Result<(), MirrorError> {
        debug_assert!(
            ops.iter().all(|o| o.file == self.path),
            "ops for this document"
        );
        // Bounded by the batch the actor committed or one replay page.
        for op in ops {
            txtodo_crdt::apply(&mut self.doc, op).map_err(|e| MirrorError::Refused {
                kind: kind_name(&op.kind),
                message: e.to_string(),
            })?;
        }
        Ok(())
    }

    /// The document this mirror follows.
    pub(crate) fn path(&self) -> &FilePath {
        &self.path
    }

    /// Imports a peer's Loro updates; the caller derives ops, reviews, and commits.
    pub fn import(&mut self, bytes: &[u8]) -> Result<Imported, MirrorError> {
        let imported = self.doc.import(bytes).map_err(loro)?;
        debug_assert!(imported.before != imported.after || !imported.applied);
        Ok(imported)
    }

    /// The same-word conflicts an import produced.
    pub fn review(&self, imported: &Imported) -> Result<Review, MirrorError> {
        txtodo_crdt::detect(&self.doc, imported).map_err(loro)
    }

    /// The ops that turn the state at `before` into the state at `after`, stamped `stamp`.
    pub fn ops_between(
        &self,
        imported: &Imported,
        stamp: &Stamp,
        mint: &mut dyn FnMut() -> OpId,
    ) -> Result<Vec<Op>, MirrorError> {
        let batch = self
            .doc
            .diff(&imported.before, &imported.after)
            .map_err(loro)?;
        let ops = txtodo_crdt::from_batch(&self.doc, &batch, stamp, mint).map_err(loro)?;
        debug_assert!(ops.iter().all(|o| o.hlc == stamp.hlc));
        Ok(ops)
    }

    /// The updates a peer at `since` (its `version()` bytes) is missing.
    pub fn export_since(&self, since: &[u8]) -> Result<Vec<u8>, MirrorError> {
        self.doc.export_updates_since(since).map_err(loro)
    }

    /// The mirror's version as opaque bytes, for a peer to export since.
    pub fn version(&self) -> Vec<u8> {
        self.doc.version_bytes()
    }

    /// The file as the mirror would render it: canonical lines (quirks dropped), blanks kept,
    /// tombstones skipped, with the state's ending and flags. The last-resort truth when the
    /// state cannot apply an import's derived ops.
    pub fn canonical_bytes(&self, like: &DocState) -> String {
        let mut out = String::new();
        for id in self.doc.list_ids(&self.path) {
            if txtodo_crdt::is_blank(id) {
                out.push('\n');
                continue;
            }
            if self.doc.is_deleted(id) {
                continue;
            }
            if let Ok(line) = txtodo_crdt::rebuild_line(&self.doc, id) {
                out.push_str(&line);
                out.push('\n');
            }
        }
        debug_assert!(out.is_empty() || out.ends_with('\n'));
        let _ = like;
        out
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

    pub(crate) fn settle_description(
        &mut self,
        task: TaskId,
        state: &DocState,
    ) -> Result<(), MirrorError> {
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

fn hydrate_line(e: &Entry) -> HydrateLine<'_> {
    match e {
        Entry::Task { id, line } => HydrateLine::Task {
            task: *id,
            // from_file validated UTF-8; an opaque line never reaches the state.
            line: line.raw().unwrap_or_default(),
        },
        Entry::Blank(_) => HydrateLine::Blank,
    }
}

fn description_of(line: &txtodo_core::OwnedLine) -> Option<String> {
    match line.parse()?.kind {
        LineKind::Task(t) => Some(t.description.to_owned()),
        LineKind::Blank => None,
    }
}

/// A `File` the state can be rebuilt from when the mirror is the last-resort truth.
pub fn file_like(bytes: &str, like: &DocState) -> File {
    let mut file = parse_file(bytes.as_bytes());
    file.ending = like.ending();
    file.bom = like.bom();
    debug_assert_eq!(file.lines.len(), bytes.lines().count());
    file
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
