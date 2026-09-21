//! `<root>/txtodo.toml`: where a workspace keeps its root list and the folder for its `ref:` lines
//! (task `workspace-layout`, decided 2026-09-20: a file at the workspace root, so the layout is
//! visible, diffable and travels with the workspace).
//!
//! ```toml
//! refs_dir  = "tasks"     # optional; "." puts ref dirs beside the list (ADR 0012's layout)
//! todo_file = "todo.txt"  # optional
//! ```
//!
//! A missing file means the defaults. A bad file is never fatal: the caller keeps the last good
//! layout. Ref: <https://docs.rs/toml/latest/toml/fn.from_str.html>

use serde::Deserialize;
use std::path::Path;
use txtodo_model::WorkspaceLayout;

/// The layout file's name, at the workspace root.
pub const LAYOUT_FILE: &str = "txtodo.toml";

/// What reading the layout file found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LayoutFile {
    /// No such file: the defaults apply.
    Missing,
    /// A valid layout.
    Valid(WorkspaceLayout),
    /// Unreadable, not TOML, or a layout this build refuses; the message says why.
    Invalid(String),
}

#[derive(Deserialize, Default)]
struct Raw {
    refs_dir: Option<String>,
    todo_file: Option<String>,
}

/// Parses a layout file's text. Unknown keys are ignored, so a newer file still loads.
pub fn parse(text: &str) -> Result<WorkspaceLayout, String> {
    let raw: Raw = toml::from_str(text).map_err(|e| format!("{LAYOUT_FILE}: {e}"))?;
    let layout = WorkspaceLayout::new(
        raw.refs_dir.as_deref().unwrap_or_default(),
        raw.todo_file.as_deref().unwrap_or_default(),
    )
    .map_err(|e| format!("{LAYOUT_FILE}: {e}"))?;
    supported(layout)
}

/// This build reads a root list only from `todo.txt`: the watcher, the walker and every client name
/// that file. A different `todo_file` is valid data but not yet honoured, so it is refused with a
/// message instead of being half applied.
pub(crate) fn supported(layout: WorkspaceLayout) -> Result<WorkspaceLayout, String> {
    if layout.todo_file() == WorkspaceLayout::default().todo_file() {
        Ok(layout)
    } else {
        Err(format!(
            "{LAYOUT_FILE}: todo_file = {:?} is not supported yet; the root list is todo.txt",
            layout.todo_file()
        ))
    }
}

/// Reads `<root>/txtodo.toml`.
pub fn read(root: &Path) -> LayoutFile {
    match std::fs::read_to_string(root.join(LAYOUT_FILE)) {
        Ok(text) => parse(&text).map_or_else(LayoutFile::Invalid, LayoutFile::Valid),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => LayoutFile::Missing,
        Err(e) => LayoutFile::Invalid(format!("{LAYOUT_FILE}: {e}")),
    }
}

/// The layout a workspace starts with: the file's when it is valid, else the defaults (a bad file
/// is logged, never fatal).
pub fn initial(root: &Path) -> WorkspaceLayout {
    match read(root) {
        LayoutFile::Valid(layout) => layout,
        LayoutFile::Missing => WorkspaceLayout::default(),
        LayoutFile::Invalid(why) => {
            log_invalid(root, &why);
            WorkspaceLayout::default()
        }
    }
}

fn log_invalid(root: &Path, why: &str) {
    tracing::warn!(root = %root.display(), why, "layout_file_invalid");
}
