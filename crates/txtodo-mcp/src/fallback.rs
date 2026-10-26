//! The note a tool result carries when the call fell back to the default workspace (task
//! two-kinds-of-workspace).
//!
//! `main.rs` aims a server at the folder it started in. When that folder is no registered
//! workspace, a call that names none lands in the daemon's default list. That used to be said on
//! stderr only, which an agent host keeps out of the model's view: an edit meant for a repo went to
//! the default list and nothing in the result said so. The note rides as a second text block, the
//! shape `todo_list` already uses for its next-page offset, so a client that reads only the first
//! block is unchanged.
//! Ref: <https://modelcontextprotocol.io/specification/2025-06-18/server/tools#tool-result>

use std::path::Path;
use std::sync::Arc;

use rmcp::model::{CallToolResponse, ContentBlock};
use serde_json::{Map, Value};

use crate::backend::WorkspaceInfo;
use crate::schema::McpServer;

/// Whether a server started in `dir` serves it, not the default workspace: the daemon already knows
/// `dir`, or `dir` is a git checkout's root. A repo is carried by git and never offered to a peer
/// (ADR 0043), so naming it to the daemon, which registers a folder on the first call that names
/// it, tells nobody its name; that was the worry that kept this server from registering anything
/// (task mcp-cwd-autoregister). A plain folder still falls back, with a note.
///
/// The registry stores canonicalized roots, so `dir` is compared both as given and canonicalized
/// (macOS's `/tmp` is `/private/tmp`). `Path::exists` is true for a `.git` folder and for the
/// `.git` file of a linked worktree.
/// Ref: <https://doc.rust-lang.org/std/path/struct.Path.html#method.exists>
pub fn serves_here(known: &[WorkspaceInfo], dir: &Path) -> bool {
    let canon = dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf());
    let registered = known
        .iter()
        .map(|w| Path::new(&w.root))
        .any(|root| root == dir || root == canon);
    registered || dir.join(".git").exists()
}

impl McpServer {
    /// Ends every result of a call that named no `workspace` with `note`, for a server that fell
    /// back to the default workspace. `None` (the default) adds nothing.
    #[must_use]
    pub fn with_fallback_note(mut self, note: Option<String>) -> McpServer {
        self.fallback_note = note.map(Arc::from);
        self
    }
}

/// Whether a call's arguments name a workspace: a non-empty string `workspace`. A call that does is
/// not a fallback, whatever the server defaulted to at startup.
pub(crate) fn names_workspace(arguments: Option<&Map<String, Value>>) -> bool {
    arguments
        .and_then(|a| a.get("workspace"))
        .and_then(Value::as_str)
        .is_some_and(|w| !w.is_empty())
}

/// `response` with `note` as a last text block, when there is a note and the call named no
/// workspace. Anything but a completed result (input required, a task) passes through.
pub(crate) fn add_note(
    response: CallToolResponse,
    note: Option<&str>,
    named_workspace: bool,
) -> CallToolResponse {
    match (response, note) {
        (CallToolResponse::Complete(mut result), Some(note)) if !named_workspace => {
            result.content.push(ContentBlock::text(note.to_owned()));
            CallToolResponse::Complete(result)
        }
        (response, _) => response,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rmcp::model::CallToolResult;
    use serde_json::json;

    fn complete() -> CallToolResponse {
        CallToolResponse::Complete(CallToolResult::success(vec![ContentBlock::text("rows")]))
    }

    fn blocks(response: &CallToolResponse) -> usize {
        match response {
            CallToolResponse::Complete(r) => r.content.len(),
            _ => 0,
        }
    }

    fn args(value: Value) -> Map<String, Value> {
        value.as_object().cloned().unwrap_or_default()
    }

    fn known(root: &Path) -> Vec<WorkspaceInfo> {
        vec![WorkspaceInfo {
            id: "01M2CZ00000000000000000A".into(),
            root: root.display().to_string(),
            added_at_ms: 0,
            root_exists: true,
            has_state: true,
            name: String::new(),
            is_list: false,
        }]
    }

    #[test]
    fn a_registered_folder_and_a_git_root_are_served_and_any_other_is_not() {
        let (plain, repo) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        std::fs::create_dir(repo.path().join(".git")).unwrap();
        assert!(
            !serves_here(&[], plain.path()),
            "a plain unknown folder falls back"
        );
        assert!(
            serves_here(&known(plain.path()), plain.path()),
            "registered"
        );
        assert!(
            serves_here(&[], repo.path()),
            "a git root registers on its first call"
        );
        let worktree = tempfile::tempdir().unwrap();
        std::fs::write(worktree.path().join(".git"), "gitdir: elsewhere\n").unwrap();
        assert!(
            serves_here(&[], worktree.path()),
            "a linked worktree's .git is a file"
        );
    }

    #[test]
    fn a_call_naming_a_workspace_is_not_a_fallback() {
        assert!(names_workspace(Some(&args(json!({"workspace": "/repo"})))));
        assert!(!names_workspace(Some(&args(json!({"workspace": ""})))));
        assert!(!names_workspace(Some(&args(json!({"workspace": null})))));
        assert!(!names_workspace(Some(&args(json!({"query": "x"})))));
        assert!(!names_workspace(None));
    }

    #[test]
    fn the_note_is_added_only_to_a_call_that_named_no_workspace() {
        let with_note = add_note(complete(), Some("n"), false);
        assert_eq!(blocks(&with_note), 2, "rows plus the note");
        assert_eq!(
            blocks(&add_note(complete(), Some("n"), true)),
            1,
            "named: untouched"
        );
        assert_eq!(
            blocks(&add_note(complete(), None, false)),
            1,
            "no note: untouched"
        );
    }
}
