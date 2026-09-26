//! Where a peer's workspace may land (sync-drift line 4): only in an empty folder.
//!
//! Joining on `PairAccept` (`WorkspaceCatalog::adopt_offered_workspace_id`, which rekeys a folder
//! in place) and mirroring an offer (`WorkspaceCatalog::mirror_workspace`) both make a folder hold
//! the peer's workspace. Lines already in that folder were minted under ids of their own, so the
//! peer's copy of the same lines lands beside them as duplicates. Both refuse such a folder.
//!
//! "Empty" means nothing sync would merge: every document the walker finds (each `todo.txt`, the
//! root list `txtodo.toml` names, each `notes.md`) holds nothing but whitespace. Other files (a
//! README, `.git`) and `.txtodo/` don't count, since they never sync. A document that can't be read
//! counts as text, and a walk that fails is not "empty": when in doubt, refuse.

use crate::layout_file::{self, LayoutFile};
use crate::walker::{self, WalkError};
use std::path::{Path, PathBuf};
use tonic::Status;
use txtodo_model::FilePath;

/// `Ok(())` when `root` is empty; else `FAILED_PRECONDITION` saying `action` (what was refused)
/// into `root`, the first document with text, and `instead` (what to do about it).
pub(crate) fn require_empty(root: &Path, action: &str, instead: &str) -> Result<(), Status> {
    let root_text = root.display();
    match first_document_with_text(root) {
        Ok(None) => Ok(()),
        Ok(Some(doc)) => Err(Status::failed_precondition(format!(
            "{action} into {root_text}: its {doc} already has text. Those lines have ids of their \
             own, so the other device's copy of them would land beside them as duplicates. \
             {instead}"
        ))),
        Err(e) => Err(Status::failed_precondition(format!(
            "{action} into {root_text}: cannot tell whether it is empty ({e}). {instead}"
        ))),
    }
}

/// The first document under `root` (walk order) with any text in it; `None` when it is empty.
fn first_document_with_text(root: &Path) -> Result<Option<FilePath>, WalkError> {
    let found = walker::walk_with(root, root_list(root).as_deref())?;
    // Unreadable counts as text. `map_or`, since `Result` has no `is_err_or`.
    // Ref: https://doc.rust-lang.org/std/result/enum.Result.html#method.map_or
    let with_text = |rel: &FilePath| {
        std::fs::read(root.join(rel.as_str())).map_or(true, |bytes| has_text(&bytes))
    };
    Ok(found.into_iter().find(with_text))
}

/// The root list's path when `txtodo.toml` names one the walker would not find by name (the same
/// rule as `Workspace::extra_document`). Read from disk, so it works for a folder not open yet.
fn root_list(root: &Path) -> Option<PathBuf> {
    match layout_file::read(root) {
        LayoutFile::Valid(layout) => layout.custom_root_list().map(|p| root.join(p.as_str())),
        LayoutFile::Missing | LayoutFile::Invalid(_) => None,
    }
}

/// Anything but whitespace and a leading byte-order mark. `trim` does not strip U+FEFF, and
/// `from_utf8_lossy` turns bad bytes into U+FFFD, which counts as text.
/// Ref: https://doc.rust-lang.org/std/primitive.str.html#method.trim
/// Ref: https://doc.rust-lang.org/std/string/struct.String.html#method.from_utf8_lossy
fn has_text(bytes: &[u8]) -> bool {
    let text = String::from_utf8_lossy(bytes);
    !text.trim_start_matches('\u{feff}').trim().is_empty()
}

#[cfg(test)]
mod tests {
    use super::{first_document_with_text, has_text};

    #[test]
    fn whitespace_and_a_bom_are_not_text() {
        assert!(!has_text(b""));
        assert!(!has_text(b"\n\n  \t\r\n"));
        assert!(!has_text("\u{feff}\n".as_bytes()));
        assert!(has_text(b"(A) buy milk\n"));
        assert!(has_text(&[0xff, 0xfe, b'\n']), "bad bytes are text");
    }

    #[test]
    fn a_folder_is_empty_until_a_list_or_notes_file_has_text() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("todo.txt"), "\n\n").unwrap();
        std::fs::write(root.join("README.md"), "not a document\n").unwrap();
        std::fs::create_dir_all(root.join(".txtodo")).unwrap();
        std::fs::write(root.join(".txtodo/todo.txt"), "state, never a document\n").unwrap();
        assert_eq!(first_document_with_text(root).unwrap(), None);

        std::fs::create_dir_all(root.join("tasks/a")).unwrap();
        std::fs::write(root.join("tasks/a/notes.md"), "a plan\n").unwrap();
        let doc = first_document_with_text(root).unwrap().unwrap();
        assert_eq!(doc.as_str(), "tasks/a/notes.md");
    }

    #[test]
    fn the_root_list_txtodo_toml_names_counts_too() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("txtodo.toml"), "todo_file = \"inbox.txt\"\n").unwrap();
        std::fs::write(root.join("inbox.txt"), "\n").unwrap();
        assert_eq!(first_document_with_text(root).unwrap(), None);
        std::fs::write(root.join("inbox.txt"), "(A) call mum\n").unwrap();
        let doc = first_document_with_text(root).unwrap().unwrap();
        assert_eq!(doc.as_str(), "inbox.txt");
    }
}
