//! Where a workspace keeps its root list and the folders for its `ref:` lines (task
//! `workspace-layout`, supersedes ADR 0012's default). Both are paths relative to the workspace
//! root. They decide *synced* paths, so they are validated the way every synced path is: `/`
//! separators only, never absolute, never `..`.
//!
//! Pure data and validation. Where the layout is stored (`<root>/txtodo.toml`) and how a ref dir
//! is created belong to the daemon; this crate only answers "given this layout, where does the
//! folder for slug S live".

use crate::{FilePath, FilePathError};
use std::fmt;

/// The root list's default place: `todo.txt` at the workspace root.
const DEFAULT_TODO_FILE: &str = "todo.txt";
/// The default folder for the ref dirs of lines in the root list: `<root>/tasks`.
const DEFAULT_REFS_DIR: &str = "tasks";
/// A `refs_dir` of `.` puts ref dirs beside the list file (ADR 0012's layout).
const BESIDE_THE_LIST: &str = ".";
/// The daemon's own state directory; nothing a user names may live inside it. Compared without
/// regard to case, since macOS and Windows filesystems fold it.
const STATE_DIR: &str = ".txtodo";

/// Why a layout was refused. The message names the offending value (constitution §3 errors).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LayoutError {
    /// The path is not a valid workspace-relative path.
    Path {
        /// Which setting: `refs_dir` or `todo_file`.
        setting: &'static str,
        /// The underlying reason.
        source: FilePathError,
    },
    /// A drive letter or another `:` (a Windows path, or one that cannot sync to Windows).
    Colon {
        /// Which setting.
        setting: &'static str,
        /// The value.
        value: String,
    },
    /// Under `.txtodo`, the daemon's own directory.
    InsideStateDir {
        /// Which setting.
        setting: &'static str,
        /// The value.
        value: String,
    },
    /// `todo_file` names the workspace root itself, not a file.
    NotAFile(String),
}

impl fmt::Display for LayoutError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LayoutError::Path { setting, source } => write!(f, "{setting}: {source}"),
            LayoutError::Colon { setting, value } => {
                write!(
                    f,
                    "{setting} {value:?} has a ':' (a drive letter?); use a relative path"
                )
            }
            LayoutError::InsideStateDir { setting, value } => {
                write!(
                    f,
                    "{setting} {value:?} is inside {STATE_DIR}, which is the daemon's own"
                )
            }
            LayoutError::NotAFile(v) => write!(f, "todo_file {v:?} must name a file"),
        }
    }
}

impl std::error::Error for LayoutError {}

/// A workspace's layout: the root list and the folder for its lines' ref dirs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkspaceLayout {
    refs_dir: String,
    todo_file: FilePath,
}

impl Default for WorkspaceLayout {
    /// `todo.txt` at the root, ref dirs under `tasks/`.
    fn default() -> Self {
        WorkspaceLayout {
            refs_dir: DEFAULT_REFS_DIR.to_owned(),
            // `todo.txt` is a valid path by inspection; `new("", "")` builds the same value.
            todo_file: default_todo_file(),
        }
    }
}

fn default_todo_file() -> FilePath {
    match FilePath::new(DEFAULT_TODO_FILE) {
        Ok(p) => p,
        Err(_) => unreachable!("todo.txt is a valid FilePath"),
    }
}

/// Checks the rules every synced layout path shares.
fn check(setting: &'static str, value: &str) -> Result<(), LayoutError> {
    FilePath::new(value).map_err(|source| LayoutError::Path { setting, source })?;
    if value.contains(':') {
        return Err(LayoutError::Colon {
            setting,
            value: value.to_owned(),
        });
    }
    let first = value.split('/').next().unwrap_or_default();
    if first.eq_ignore_ascii_case(STATE_DIR) {
        return Err(LayoutError::InsideStateDir {
            setting,
            value: value.to_owned(),
        });
    }
    Ok(())
}

impl WorkspaceLayout {
    /// Validates a layout. `refs_dir` may be `.` (beside the list file); `todo_file` must name a
    /// file. An empty string means "the default" for that one setting, so a `txtodo.toml` that
    /// sets only one of them keeps the other.
    pub fn new(refs_dir: &str, todo_file: &str) -> Result<WorkspaceLayout, LayoutError> {
        let refs_dir = if refs_dir.is_empty() {
            DEFAULT_REFS_DIR
        } else {
            refs_dir
        };
        let todo_file = if todo_file.is_empty() {
            DEFAULT_TODO_FILE
        } else {
            todo_file
        };
        if refs_dir != BESIDE_THE_LIST {
            check("refs_dir", refs_dir)?;
        }
        if todo_file == BESIDE_THE_LIST {
            return Err(LayoutError::NotAFile(todo_file.to_owned()));
        }
        check("todo_file", todo_file)?;
        let todo_file = FilePath::new(todo_file).map_err(|source| LayoutError::Path {
            setting: "todo_file",
            source,
        })?;
        Ok(WorkspaceLayout {
            refs_dir: refs_dir.to_owned(),
            todo_file,
        })
    }

    /// ADR 0012's layout: `todo.txt` at the root, ref dirs beside it (`refs_dir = "."`). Not the
    /// default (that is `tasks`); a caller that has not yet been given a layout, and the tests
    /// written against the old placement, use this.
    pub fn beside_the_list() -> WorkspaceLayout {
        WorkspaceLayout {
            refs_dir: BESIDE_THE_LIST.to_owned(),
            todo_file: default_todo_file(),
        }
    }

    /// The folder for the ref dirs of root-list lines, as written (`tasks`, or `.`).
    pub fn refs_dir(&self) -> &str {
        &self.refs_dir
    }

    /// The root list's path, as written (`todo.txt`).
    pub fn todo_file(&self) -> &str {
        self.todo_file.as_str()
    }

    /// The root list as a validated path.
    pub fn root_list(&self) -> FilePath {
        self.todo_file.clone()
    }

    /// True when ref dirs sit beside the list file (`refs_dir = "."`), ADR 0012's layout.
    pub fn refs_beside_list(&self) -> bool {
        self.refs_dir == BESIDE_THE_LIST
    }

    /// The directory that holds the ref dirs of the lines in `owner`, relative to the workspace
    /// root (`""` is the root itself): `refs_dir` for the root list, or the list's own folder when
    /// refs sit beside it; every other list, a nested one, keeps them beside its own file.
    pub fn refs_parent_of(&self, owner: &FilePath) -> String {
        let beside = |file: &str| match file.rsplit_once('/') {
            Some((dir, _)) => dir.to_owned(),
            None => String::new(),
        };
        if owner != &self.todo_file {
            beside(owner.as_str())
        } else if self.refs_beside_list() {
            beside(self.todo_file.as_str())
        } else {
            self.refs_dir.clone()
        }
    }

    /// The ref dir of slug `slug` for a line in `owner`, workspace-relative: `refs_parent_of`
    /// joined with the slug. The one place a slug becomes a directory (task workspace-layout).
    pub fn ref_dir_for(&self, owner: &FilePath, slug: &str) -> String {
        match self.refs_parent_of(owner).as_str() {
            "" => slug.to_owned(),
            parent => format!("{parent}/{slug}"),
        }
    }

    /// The directory that holds the ref dir of slug `slug` for a line in the root list, relative
    /// to the workspace root: `<refs_dir>/<slug>`, or `<list's folder>/<slug>` when refs sit beside
    /// the list. The slug is not validated here (`Task::ref_slug` already did).
    pub fn ref_dir_of(&self, slug: &str) -> String {
        if self.refs_beside_list() {
            match self.todo_file.as_str().rsplit_once('/') {
                Some((dir, _)) => format!("{dir}/{slug}"),
                None => slug.to_owned(),
            }
        } else {
            format!("{}/{slug}", self.refs_dir)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_is_todo_txt_and_tasks() {
        let l = WorkspaceLayout::default();
        assert_eq!((l.todo_file(), l.refs_dir()), ("todo.txt", "tasks"));
        assert_eq!(l.ref_dir_of("auth"), "tasks/auth");
        assert_eq!(l.root_list().as_str(), "todo.txt");
        assert_eq!(WorkspaceLayout::new("", "").unwrap(), l);
    }

    #[test]
    fn a_dot_refs_dir_is_adr_0012s_layout() {
        let l = WorkspaceLayout::new(".", "todo.txt").unwrap();
        assert!(l.refs_beside_list());
        assert_eq!(l.ref_dir_of("auth"), "auth");
    }

    /// The known gap in the task notes: a list in a subfolder with refs beside it.
    #[test]
    fn refs_beside_a_list_in_a_subfolder_follow_the_list() {
        let l = WorkspaceLayout::new(".", "lists/todo.txt").unwrap();
        assert_eq!(l.ref_dir_of("auth"), "lists/auth");
    }

    #[test]
    fn nested_folders_are_allowed() {
        let l = WorkspaceLayout::new("work/refs", "work/todo.txt").unwrap();
        assert_eq!(l.ref_dir_of("a"), "work/refs/a");
    }

    #[test]
    fn unsafe_and_unportable_paths_are_refused() {
        for bad in [
            "/abs",
            "../up",
            "a/../b",
            "a\\b",
            "a//b",
            "C:/x",
            "c:x",
            ".txtodo",
            ".txtodo/refs",
            ".TxTodo/refs",
        ] {
            assert!(
                WorkspaceLayout::new(bad, "todo.txt").is_err(),
                "refs_dir {bad}"
            );
            assert!(
                WorkspaceLayout::new("tasks", bad).is_err(),
                "todo_file {bad}"
            );
        }
        assert!(matches!(
            WorkspaceLayout::new("tasks", "."),
            Err(LayoutError::NotAFile(_))
        ));
    }

    #[test]
    fn every_error_names_the_setting_and_the_value() {
        let e = WorkspaceLayout::new("../up", "todo.txt")
            .unwrap_err()
            .to_string();
        assert!(e.starts_with("refs_dir:"), "{e}");
        let e = WorkspaceLayout::new("tasks", "C:/x")
            .unwrap_err()
            .to_string();
        assert!(e.contains("todo_file") && e.contains("C:/x"), "{e}");
    }

    #[test]
    fn a_ref_dir_follows_the_list_that_owns_the_line() {
        let root = FilePath::new("todo.txt").unwrap();
        let nested = FilePath::new("tasks/auth/todo.txt").unwrap();
        let l = WorkspaceLayout::default();
        assert_eq!(l.ref_dir_for(&root, "auth"), "tasks/auth");
        assert_eq!(l.ref_dir_for(&nested, "login"), "tasks/auth/login");
        let beside = WorkspaceLayout::beside_the_list();
        assert_eq!(beside.ref_dir_for(&root, "auth"), "auth");
        assert_eq!(beside.ref_dir_for(&nested, "login"), "tasks/auth/login");
        let sub = WorkspaceLayout::new(".", "lists/todo.txt").unwrap();
        assert_eq!(
            sub.ref_dir_for(&FilePath::new("lists/todo.txt").unwrap(), "a"),
            "lists/a"
        );
        assert_eq!(l.refs_parent_of(&root), "tasks");
        assert_eq!(beside.refs_parent_of(&root), "");
    }
}
