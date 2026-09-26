//! The root list's name in direct-file mode (`todo_file` in `txtodo.toml`) and the rule it must
//! pass, split out of `config.rs` for that file's line budget.

use crate::CliError;
use crate::config::Paths;
use std::path::Path;

/// Points `paths` at `list`, a list inside the workspace named by the hidden `--list` flag (what
/// `sub` passes), instead of the root list. The workspace stays `paths.dir`, so the daemon is
/// asked about the workspace that holds the list, never about the list's own folder, which it
/// would register as a second workspace (sync-drift line 3). `report.txt` goes beside the list,
/// where `sub` has always put it. Same name rules as a root list.
pub(crate) fn scope_to_list(paths: &mut Paths, list: &str) -> Result<(), CliError> {
    if !valid_root_list(list) {
        return Err(CliError::Message(format!(
            "txtodo: --list {list:?} is not a list inside the workspace (relative, `/` \
             separators, no `.`/`..`, no `:`, not under .txtodo, not notes.md)"
        )));
    }
    paths.todo = paths.dir.join(list);
    paths.todo_file = list.to_owned();
    paths.report = paths.todo.parent().unwrap_or(&paths.dir).join("report.txt");
    paths.layout_note = None;
    debug_assert!(
        paths.todo.starts_with(&paths.dir),
        "the list is in the workspace"
    );
    Ok(())
}

/// The workspace's root list for direct-file mode: `todo_file` from `<dir>/txtodo.toml`, else
/// `todo.txt`, checked by the same rules the daemon applies (`specs/ref-directories.md` rule 2,
/// task layout-toml-validation) — a name that fails them is ignored, with a note the CLI prints,
/// rather than written to (`todo_file = "../elsewhere/todo.txt"` used to escape the workspace).
/// In daemon mode `commands::layout::adopt_root_list` replaces this answer with the daemon's.
/// Ref: <https://docs.rs/toml/latest/toml/fn.from_str.html>
pub(crate) fn root_list_name(dir: &Path) -> (String, Option<String>) {
    #[derive(serde::Deserialize)]
    struct Layout {
        todo_file: Option<String>,
    }
    let named = std::fs::read_to_string(dir.join("txtodo.toml"))
        .ok()
        .and_then(|text| toml::from_str::<Layout>(&text).ok())
        .and_then(|l| l.todo_file)
        .filter(|f| !f.is_empty());
    match named {
        Some(name) if valid_root_list(&name) => (name, None),
        Some(name) => (
            "todo.txt".to_owned(),
            Some(format!(
                "txtodo: ignoring todo_file = {name:?} in txtodo.toml: not a valid root list name \
                 (relative, `/` separators, no `.`/`..`, no `:`, not under .txtodo, not notes.md); \
                 using todo.txt"
            )),
        ),
        None => ("todo.txt".to_owned(), None),
    }
}

/// The root-list rules of `specs/ref-directories.md` rule 2, as the daemon's
/// `WorkspaceLayout::new` checks them (`txtodo-model`, which this crate may not depend on):
/// relative, `/` separators only, no empty, `.` or `..` component, no `:`, not under `.txtodo`,
/// and a file that is not `notes.md`.
pub(crate) fn valid_root_list(name: &str) -> bool {
    if name.is_empty() || name.starts_with('/') || name.ends_with('/') || name.contains('\\') {
        return false;
    }
    if name.contains(':') {
        return false;
    }
    let mut parts = name.split('/');
    let first = parts.next().unwrap_or_default();
    if first.eq_ignore_ascii_case(".txtodo") {
        return false;
    }
    if name
        .split('/')
        .any(|p| p.is_empty() || p == "." || p == "..")
    {
        return false;
    }
    name.rsplit('/').next() != Some("notes.md")
}

#[cfg(test)]
mod root_list_tests {
    use super::*;

    #[test]
    fn the_root_list_rules_match_the_daemons() {
        for ok in ["todo.txt", "work.txt", "lists/work.txt", "a.b/c.txt"] {
            assert!(valid_root_list(ok), "{ok}");
        }
        for bad in [
            "",
            "/etc/todo.txt",
            "../elsewhere/todo.txt",
            "lists/../todo.txt",
            "./todo.txt",
            "lists//todo.txt",
            "lists/",
            "c:todo.txt",
            ".txtodo/todo.txt",
            ".TXTODO/x.txt",
            "notes.md",
            "tasks/notes.md",
            "lists\\work.txt",
        ] {
            assert!(!valid_root_list(bad), "{bad}");
        }
    }

    #[test]
    fn a_bad_todo_file_is_ignored_with_a_note_and_a_good_one_is_used() {
        let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        std::fs::write(
            dir.path().join("txtodo.toml"),
            "todo_file = \"../escape/todo.txt\"\n",
        )
        .unwrap_or_else(|e| panic!("{e}"));
        let (name, note) = root_list_name(dir.path());
        assert_eq!(name, "todo.txt");
        assert!(note.is_some_and(|n| n.contains("ignoring todo_file")));
        std::fs::write(dir.path().join("txtodo.toml"), "todo_file = \"work.txt\"\n")
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(root_list_name(dir.path()), ("work.txt".to_owned(), None));
        std::fs::remove_file(dir.path().join("txtodo.toml")).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(root_list_name(dir.path()), ("todo.txt".to_owned(), None));
    }
}
