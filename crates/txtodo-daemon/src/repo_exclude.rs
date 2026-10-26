//! Keeping `.txtodo/` out of a repo's git (task `two-kinds-of-workspace`, ADR 0043). The daemon
//! drops its op log in `<repo>/.txtodo/`; untracked, it sits one `git add -A` from being committed.
//! The fix is a line in the clone's own `info/exclude`, which git never shares and nothing tracked
//! records, written before the daemon first registers the repo.
//!
//! Done without running `git` (the daemon's `PATH` may not have it): `.git` is a folder in a clone,
//! and in a linked worktree or a submodule a file `gitdir: <path>` that leads to the git folder,
//! and from a worktree's on to the shared one named by its `commondir`.
//! Ref: <https://git-scm.com/docs/gitrepository-layout>, <https://git-scm.com/docs/gitignore>

use std::io::Write;
use std::path::{Path, PathBuf};

/// The git folder the clone at `root` shares its `config` and `info/` with, or `None` when `root` has
/// no `.git`: `.git` itself in a clone, and for a linked worktree the common folder its `commondir`
/// names.
pub(crate) fn git_common_dir(root: &Path) -> Option<PathBuf> {
    let git = root.join(".git");
    if git.is_dir() {
        return Some(git);
    }
    let text = std::fs::read_to_string(&git).ok()?;
    let gitdir = PathBuf::from(text.strip_prefix("gitdir:")?.trim());
    let gitdir = if gitdir.is_absolute() {
        gitdir
    } else {
        root.join(gitdir)
    };
    Some(
        std::fs::read_to_string(gitdir.join("commondir"))
            .map_or(gitdir.clone(), |rel| gitdir.join(rel.trim())),
    )
}

/// Where git reads the exclude file of the clone at `root`, or `None` when `root` has no `.git`.
pub(crate) fn info_exclude(root: &Path) -> Option<PathBuf> {
    git_common_dir(root).map(|dir| dir.join("info").join("exclude"))
}

/// Adds `.txtodo/` to the exclude file of the clone at `root` unless it is there already; `true`
/// when it wrote. Anything else in the file is kept.
pub(crate) fn exclude_txtodo(root: &Path) -> std::io::Result<bool> {
    let Some(path) = info_exclude(root) else {
        return Ok(false);
    };
    let text = std::fs::read_to_string(&path).unwrap_or_default();
    if text
        .lines()
        .any(|l| matches!(l.trim(), ".txtodo/" | ".txtodo"))
    {
        return Ok(false);
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)?;
    if !text.is_empty() && !text.ends_with('\n') {
        file.write_all(b"\n")?;
    }
    file.write_all(b".txtodo/\n")?;
    Ok(true)
}

#[cfg(test)]
#[path = "repo_exclude_tests.rs"]
mod tests;
