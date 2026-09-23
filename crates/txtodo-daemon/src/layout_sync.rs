//! `<root>/txtodo.toml` syncs like `notes.md` (task workspace-layout, decided 2026-09-20): the
//! file is one more whole-text document owned by a `NotesActor` under the path `txtodo.toml`, so
//! its bytes travel as `NotesEdit` ops the way a hand-written notes.md does (task notes-sync).
//!
//! Three seams, and no new wire format: discovery seeds the actor when the file exists
//! (`Workspace::register_discovered`), so a peer's `Want` can fetch it; the layout RPC and the
//! watcher's reload record the bytes on disk as an op after any change (`record_disk`), a no-op
//! when the actor already holds them (a peer's import wrote them); and `lan_apply` routes a
//! peer's ops for this path through the notes actor, whose commit writes the file, which the
//! watcher then hot-reloads (`layout_reload.rs`). A change refused there (ref dirs in the old
//! place) still lands on disk: both devices hold one file, and the note says why it is not yet
//! the layout in force.

use crate::handle::ActorError;
use crate::layout_file::LAYOUT_FILE;
use crate::workspace::Workspace;
use std::sync::PoisonError;
use txtodo_model::{FilePath, Principal};

/// The layout file as a document path; `None` only if `LAYOUT_FILE` ever stopped being a valid
/// relative path, which a unit test pins.
fn layout_path() -> Option<FilePath> {
    FilePath::new(LAYOUT_FILE).ok()
}

/// True for the root layout file and nothing else: a `txtodo.toml` deeper down is not a layout.
pub(crate) fn is_layout_document(path: &FilePath) -> bool {
    path.as_str() == LAYOUT_FILE
}

/// Opens the layout file's actor at discovery when the file exists, so its bytes are in the op
/// log before any peer asks. A workspace with no file mints nothing.
pub(crate) fn seed_layout(ws: &Workspace) {
    if !ws.root().join(LAYOUT_FILE).is_file() {
        return;
    }
    if let Some(path) = layout_path() {
        ws.seed_notes(&path);
    }
}

/// Records what `<root>/txtodo.toml` now holds as one `NotesEdit` op from `principal`, or nothing
/// when the actor already holds those bytes. A missing file records nothing: opening its actor
/// would write an empty file back, which reads as the default layout, while `layout_reload`
/// keeps the last good one on a deletion — so a deletion stays on the device that made it (a
/// known gap, in the task notes). Never fatal: a failure is logged and the layout is unaffected.
pub(crate) fn record_disk(ws: &Workspace, principal: Principal) {
    let Some(path) = layout_path() else {
        return;
    };
    let Ok(text) = std::fs::read_to_string(ws.root().join(LAYOUT_FILE)) else {
        return;
    };
    let outcome = ws.notes_actor(&path).and_then(|cell| {
        let mut actor = cell.lock().unwrap_or_else(PoisonError::into_inner);
        actor.edit(&text, principal).map(|applied| applied.applied)
    });
    if let Err(e) = outcome {
        log_failed(&e);
    }
}

/// Its own function for the cognitive-complexity budget: a `tracing` macro counts.
fn log_failed(e: &ActorError) {
    tracing::warn!(file = LAYOUT_FILE, error = %e, "layout_sync_record_failed");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_layout_file_is_a_valid_document_path_and_only_at_the_root() {
        let path = layout_path().unwrap_or_else(|| panic!("txtodo.toml is a valid FilePath"));
        assert!(is_layout_document(&path));
        let nested = FilePath::new("tasks/x/txtodo.toml").unwrap_or_else(|e| panic!("{e}"));
        assert!(!is_layout_document(&nested));
    }
}
