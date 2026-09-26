//! Which folders a workspace's walk reaches, and so whether two workspace roots share lists (task
//! sync-drift line 3). One rule for the daemon, which walks and registers workspaces, and for the
//! CLI's `doctor`, which checks the registry, so the two never disagree.
//!
//! Every `todo.txt` under a workspace root is one of its lists (`specs/ref-directories.md` rule
//! 11). A second workspace registered inside the first, or around it, tracks the same files in a
//! second store with its own ids, and each store reads the other's writes as outside edits. That
//! holds only where the walk goes: `.txtodo/`, a nested git checkout (a linked worktree) and the
//! other skipped folders are never walked, so a workspace there shares nothing.

use std::path::Path;

/// The daemon's own state folder in every workspace: never walked.
const STATE_DIR: &str = ".txtodo";

/// Folder names a workspace walk never enters wherever they appear: version-control internals and
/// installed dependencies, which hold thousands of files and never a todo list.
pub const SKIPPED_DIR_NAMES: [&str; 2] = [".git", "node_modules"];

/// True for a directory that is never part of a workspace's documents: `.git` and `node_modules`;
/// another git checkout (it holds a `.git` dir, or a `.git` file for a linked worktree or a
/// submodule: the boundary [`crate::workspace_root_from`] stops at too, task
/// walker-nested-checkouts); a Cargo build directory (`target` holding the `CACHEDIR.TAG` cargo
/// writes into it, or sitting beside a `Cargo.toml`); and `.claude/worktrees`, whose checkouts are
/// whole copies of the repo (registered as workspaces of their own when they matter, never a
/// subtree of this one). Walking them made the daemon adopt about 2979 documents against 361 real
/// ones and re-walk every new directory a `cargo build` created under `target/` (root todo
/// id:01M2WK7W1MPDW9VBWS25EF8CB5). A ref directory that merely happens to be called `target` is
/// still walked. Moved here from `txtodo-daemon`'s walker so [`walks_into`] uses the same rule.
/// Ref: <https://bford.info/cachedir/>, <https://git-scm.com/docs/gitrepository-layout>
pub fn is_skipped_dir(dir: &Path) -> bool {
    let Some(name) = dir.file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    // `symlink_metadata` does not follow links, so a dangling `.git` symlink still counts.
    // Ref: https://doc.rust-lang.org/std/fs/fn.symlink_metadata.html
    if SKIPPED_DIR_NAMES.contains(&name) || std::fs::symlink_metadata(dir.join(".git")).is_ok() {
        return true;
    }
    let parent = dir.parent();
    match name {
        "target" => {
            dir.join("CACHEDIR.TAG").is_file()
                || parent.is_some_and(|p| p.join("Cargo.toml").is_file())
        }
        "worktrees" => parent.and_then(Path::file_name).and_then(|n| n.to_str()) == Some(".claude"),
        _ => false,
    }
}

/// Whether a workspace rooted at `outer` walks into `inner`: `inner` is strictly below `outer`,
/// and the walk enters every folder on the way down, `inner` included (none is `.txtodo` or
/// [`is_skipped_dir`]). Pass canonical paths, as registered roots are: the test is by path
/// component, so `/a/bc` is not below `/a/b`, and no symlink is resolved here.
/// Ref: <https://doc.rust-lang.org/std/path/struct.Path.html#method.strip_prefix>
pub fn walks_into(outer: &Path, inner: &Path) -> bool {
    let Ok(below) = inner.strip_prefix(outer) else {
        return false;
    };
    if below.as_os_str().is_empty() {
        return false;
    }
    let mut dir = outer.to_path_buf();
    // Bounded by `inner`'s own component count.
    below.components().all(|part| {
        dir.push(part);
        part.as_os_str() != STATE_DIR && !is_skipped_dir(&dir)
    })
}

/// How a workspace root sits against another one when the two would share lists.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RootOverlap {
    /// The root is inside the other workspace, whose walk already reaches its lists.
    Inside,
    /// The root holds the other workspace: its walk would reach that workspace's lists.
    Around,
}

/// How `root` overlaps `other`, if at all. The same folder is no overlap: registration already
/// dedupes an exact root on its own.
pub fn root_overlap(root: &Path, other: &Path) -> Option<RootOverlap> {
    if walks_into(other, root) {
        Some(RootOverlap::Inside)
    } else if walks_into(root, other) {
        Some(RootOverlap::Around)
    } else {
        None
    }
}

#[cfg(test)]
#[path = "walk_scope_tests.rs"]
mod tests;
