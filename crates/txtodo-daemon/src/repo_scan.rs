//! Finding git clones that hold a todo list (task `two-kinds-of-workspace`, ADR 0043): a pure walk
//! under the scan roots. A **repo** here is a folder with a `.git` *directory* whose top holds a
//! `todo.txt` or a `txtodo.toml`. A linked worktree or a submodule has a `.git` *file*; the scan
//! leaves those to a first touch (`txtodo add` in one registers it at once).
//!
//! Same shape as `walker.rs`: an explicit stack and `read_dir`, no recursion, no new dependency.
//! Bounded three ways: [`MAX_DEPTH`] levels under a root, [`MAX_DIRS`] folders read, [`MAX_FOUND`]
//! repos kept. A found repo is recorded and the walk goes on below it (nested clones are each their
//! own workspace); only `.git` itself, any hidden folder, and the folders named in [`SKIPPED`] are
//! never entered, nor is a symbolic link followed.
//! Ref: <https://doc.rust-lang.org/std/fs/fn.read_dir.html>

use std::path::{Path, PathBuf};

use txtodo_workspace_paths::SKIPPED_DIR_NAMES;

/// Levels below a scan root that are read: a clone at `root/a/b/c` is depth 3.
pub(crate) const MAX_DEPTH: usize = 8;

/// Most folders one scan reads before it stops (a runaway tree, a mounted network share).
pub(crate) const MAX_DIRS: usize = 250_000;

/// Most repos one scan keeps.
pub(crate) const MAX_FOUND: usize = 500;

/// Folder names never entered, beside hidden ones and [`SKIPPED_DIR_NAMES`]: build output and
/// vendored code, where a stray `.git` and `todo.txt` is not the person's backlog.
const SKIPPED: [&str; 4] = ["target", "vendor", "dist", "build"];

/// What a scan saw.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Scanned {
    /// The repos found, in the order the walk met them.
    pub repos: Vec<PathBuf>,
    /// Folders read.
    pub dirs: usize,
    /// The walk stopped at [`MAX_DIRS`] or [`MAX_FOUND`] before it ran out of folders.
    pub truncated: bool,
}

/// Walks under every root in `roots`, never into a folder in `skip` (the daemon's own data, say).
/// A root that is gone or unreadable is skipped. `Library` directly under a root is skipped too:
/// on macOS it is the application data of the person's home folder.
pub(crate) fn scan(roots: &[PathBuf], skip: &[PathBuf]) -> Scanned {
    let mut out = Scanned::default();
    for root in roots {
        let mut stack = vec![(root.clone(), 0usize)];
        while let Some((dir, depth)) = stack.pop() {
            if out.dirs >= MAX_DIRS || out.repos.len() >= MAX_FOUND {
                out.truncated = true;
                return out;
            }
            out.dirs += 1;
            if is_repo(&dir) && !out.repos.contains(&dir) {
                out.repos.push(dir.clone());
            }
            if depth < MAX_DEPTH {
                push_children(&dir, depth, skip, &mut stack);
            }
        }
    }
    out
}

/// A clone (a `.git` folder) whose top holds a todo list.
fn is_repo(dir: &Path) -> bool {
    dir.join(".git").is_dir()
        && (dir.join("todo.txt").is_file() || dir.join("txtodo.toml").is_file())
}

/// Pushes the folders worth entering below `dir`, in name order so a scan is repeatable.
fn push_children(dir: &Path, depth: usize, skip: &[PathBuf], stack: &mut Vec<(PathBuf, usize)>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut names: Vec<(String, PathBuf)> = entries
        .flatten()
        // `DirEntry::file_type` does not follow a symbolic link, so a link is never a folder here.
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .filter_map(|e| Some((e.file_name().into_string().ok()?, e.path())))
        .filter(|(name, path)| enter(name, depth, path, skip))
        .collect();
    names.sort();
    // Pushed in reverse, so the stack pops them in name order.
    stack.extend(names.into_iter().rev().map(|(_, path)| (path, depth + 1)));
}

fn enter(name: &str, depth: usize, path: &Path, skip: &[PathBuf]) -> bool {
    let never = name.starts_with('.')
        || SKIPPED_DIR_NAMES.contains(&name)
        || SKIPPED.contains(&name)
        || (depth == 0 && name == "Library");
    !never && !skip.iter().any(|s| path.starts_with(s))
}

#[cfg(test)]
#[path = "repo_scan_tests.rs"]
mod tests;
