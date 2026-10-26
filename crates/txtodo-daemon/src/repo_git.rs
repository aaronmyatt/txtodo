//! Making a git clone ready for txtodo with no step for the person (task `two-kinds-of-workspace`,
//! ADR 0043). A repo's todo list travels with git, so a clone needs three things, and this writes
//! each only if it is missing, inside the clone's own `.git` and nothing tracked:
//!
//! - `.txtodo/` in `info/exclude` (`repo_exclude.rs`): the daemon's op log would otherwise sit
//!   untracked beside the list, one `git add -A` from being committed;
//! - `todo.txt merge=txtodo` (the root list's name too when `txtodo.toml` gives another) and
//!   `done.txt merge=union` in `info/attributes`, which git reads like a `.gitattributes` that is
//!   per clone and never in the working tree. A line already in the clone's `.gitattributes` (what
//!   `txtodo git setup` writes, for a team that commits it) counts, and is not repeated;
//! - the merge driver in the clone's `config`: `[merge "txtodo"]` with `driver = txtodo merge-driver
//!   %O %A %B`, `txtodo` found on `PATH` when git runs it, so a Homebrew upgrade cannot leave a stale
//!   path. A `driver` already set there, by the person or by `txtodo git setup`, is left alone.
//!
//! Done by editing the files, without running `git` (the daemon's `PATH` may not have it). A clone
//! that git is writing to right now (`config.lock`) is left to the next scan. A linked worktree
//! writes to the shared git folder its `commondir` names.
//! Ref: <https://git-scm.com/docs/gitattributes#_defining_a_custom_merge_driver>,
//! <https://git-scm.com/docs/git-config#_configuration_file>, <https://git-scm.com/docs/gitrepository-layout>

use std::io::Write;
use std::path::Path;

use crate::repo_exclude::{exclude_txtodo, git_common_dir};

const HEADER: &str = "# txtodo: merge todo lists line by line (written by txtodod)";
const DRIVER: &str = "txtodo merge-driver %O %A %B";

/// What [`prepare`] wrote.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Prepared {
    /// `.txtodo/` went into `info/exclude`.
    pub(crate) exclude: bool,
    /// Lines went into `info/attributes`.
    pub(crate) attributes: bool,
    /// The merge driver went into `config`.
    pub(crate) driver: bool,
}

impl Prepared {
    /// Whether anything was written.
    pub(crate) fn any(self) -> bool {
        self.exclude || self.attributes || self.driver
    }
}

/// Readies the clone at `root`; nothing for a folder with no `.git`. Safe to run again: the second
/// call writes nothing.
pub(crate) fn prepare(root: &Path) -> std::io::Result<Prepared> {
    let Some(git) = git_common_dir(root) else {
        return Ok(Prepared::default());
    };
    Ok(Prepared {
        exclude: exclude_txtodo(root)?,
        attributes: add_attributes(root, &git)?,
        driver: add_driver(&git)?,
    })
}

/// The attribute lines the lists need: the root list (and `todo.txt`), and `done.txt`.
fn wanted(root: &Path) -> Vec<String> {
    let mut lines = vec!["todo.txt merge=txtodo".to_owned()];
    let custom = crate::join_target::root_list(root)
        .and_then(|p| p.strip_prefix(root).ok().map(|r| r.display().to_string()));
    if let Some(name) = custom.filter(|n| n != "todo.txt") {
        lines.push(format!("{name} merge=txtodo"));
    }
    lines.push("done.txt merge=union".to_owned());
    lines
}

fn add_attributes(root: &Path, git: &Path) -> std::io::Result<bool> {
    let path = git.join("info").join("attributes");
    let here = std::fs::read_to_string(&path).unwrap_or_default();
    let committed = std::fs::read_to_string(root.join(".gitattributes")).unwrap_or_default();
    let has = |line: &str| {
        [&here, &committed]
            .iter()
            .any(|text| text.lines().any(|l| l.trim() == line))
    };
    let missing: Vec<String> = wanted(root).into_iter().filter(|l| !has(l)).collect();
    if missing.is_empty() {
        return Ok(false);
    }
    let mut block = String::new();
    if !here.is_empty() && !here.ends_with('\n') {
        block.push('\n');
    }
    if !here.contains(HEADER) {
        block.push_str(HEADER);
        block.push('\n');
    }
    for line in &missing {
        block.push_str(line);
        block.push('\n');
    }
    append(&path, &block)?;
    Ok(true)
}

fn add_driver(git: &Path) -> std::io::Result<bool> {
    let path = git.join("config");
    if git.join("config.lock").exists() {
        return Ok(false);
    }
    let text = std::fs::read_to_string(&path).unwrap_or_default();
    if has_driver(&text) {
        return Ok(false);
    }
    let mut block = String::new();
    if !text.is_empty() && !text.ends_with('\n') {
        block.push('\n');
    }
    block.push_str(&format!(
        "[merge \"txtodo\"]\n\tname = txtodo todo lists\n\tdriver = {DRIVER}\n"
    ));
    append(&path, &block)?;
    Ok(true)
}

/// Whether the config text sets `merge.txtodo.driver`: a `[merge "txtodo"]` section holding a
/// `driver` key. Section names are case-insensitive, the subsection is not.
fn has_driver(config: &str) -> bool {
    let mut in_section = false;
    for line in config.lines().map(str::trim) {
        if line.starts_with('[') {
            in_section = is_txtodo_merge_header(line);
        } else if in_section {
            let key = line.split(['=', ' ', '\t']).next().unwrap_or("");
            if key.eq_ignore_ascii_case("driver") {
                return true;
            }
        }
    }
    false
}

fn is_txtodo_merge_header(line: &str) -> bool {
    let Some(rest) = line.strip_prefix('[') else {
        return false;
    };
    let mut parts = rest.trim_end_matches(']').splitn(2, char::is_whitespace);
    let name = parts.next().unwrap_or("");
    let sub = parts.next().unwrap_or("").trim();
    name.eq_ignore_ascii_case("merge") && sub == "\"txtodo\""
}

fn append(path: &Path, text: &str) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    file.write_all(text.as_bytes())
}

#[cfg(test)]
#[path = "repo_git_tests.rs"]
mod tests;
