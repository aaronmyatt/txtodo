//! Moving one workspace's copy aside for a rejoin (task sync-drift line 8). Nothing here deletes a
//! user's file: `.txtodo/`, `txtodo.toml` and every synced document are renamed into a new folder
//! beside the root, named for the moment it ran, and a failure moves back what already moved.
//!
//! `.txtodo/` goes first, and a plain file of that name (the guard) takes its place until the rest
//! has moved: `Workspace::open` cannot make its state folder over a file, so a crash half way
//! leaves a folder no open accepts. The other order is worse. An old store beside a missing list
//! can turn into deletes a peer takes; no store beside old lines only re-mints them.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, UNIX_EPOCH};

use crate::layout_file::LAYOUT_FILE;
use crate::walker::{self, STATE_DIR};

/// The guard's first line, so only a file this module wrote is ever removed as one.
const GUARD_HEAD: &str = "txtodo rejoin in progress";

/// What a rejoin moves, and where to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Plan {
    pub(crate) root: PathBuf,
    /// The new folder beside `root`; not made until [`move_aside`].
    pub(crate) backup: PathBuf,
    /// Whether `root/.txtodo` exists (a folder, or a guard left by a rejoin that crashed).
    pub(crate) state: bool,
    /// Root-relative, `/`-separated, in move order: `txtodo.toml` when present, then each document.
    pub(crate) documents: Vec<String>,
}

impl Plan {
    /// Everything that moves, `.txtodo` first: what a rejoin reports.
    pub(crate) fn entries(&self) -> Vec<String> {
        let state = self.state.then(|| STATE_DIR.to_owned());
        state
            .into_iter()
            .chain(self.documents.iter().cloned())
            .collect()
    }
}

/// What `root` would move right now, and where to. Reads only.
pub(crate) fn plan(root: &Path, now_ms: u64) -> Result<Plan, String> {
    let backup = backup_dir(root, now_ms)?;
    let mut documents = Vec::new();
    if root.join(LAYOUT_FILE).is_file() {
        documents.push(LAYOUT_FILE.to_owned());
    }
    // The root list `txtodo.toml` names, found before that file moves.
    let root_list = crate::join_target::root_list(root);
    let found = walker::walk_with(root, root_list.as_deref())
        .map_err(|e| format!("list the documents under {}: {e}", root.display()))?;
    documents.extend(found.iter().map(|d| d.as_str().to_owned()));
    Ok(Plan {
        root: root.to_path_buf(),
        backup,
        // `symlink_metadata`: whatever is there counts, even a dangling link.
        // Ref: https://doc.rust-lang.org/std/fs/fn.symlink_metadata.html
        state: std::fs::symlink_metadata(root.join(STATE_DIR)).is_ok(),
        documents,
    })
}

/// `<parent>/<name>.rejoin-backup-<UTC time>`, the first such name not taken yet.
fn backup_dir(root: &Path, now_ms: u64) -> Result<PathBuf, String> {
    let (Some(parent), Some(name)) = (root.parent(), root.file_name()) else {
        return Err(format!(
            "{} has no parent folder for a backup",
            root.display()
        ));
    };
    let base = format!("{}.rejoin-backup-{}", name.to_string_lossy(), stamp(now_ms));
    (1..=9)
        .map(|n| match n {
            1 => parent.join(&base),
            n => parent.join(format!("{base}-{n}")),
        })
        .find(|p| std::fs::symlink_metadata(p).is_err())
        .ok_or_else(|| format!("{}/{base} and 8 more names are taken", parent.display()))
}

/// RFC 3339 in UTC, to the second, without the colons Finder shows as slashes.
/// Ref: https://docs.rs/humantime/latest/humantime/fn.format_rfc3339_seconds.html
fn stamp(now_ms: u64) -> String {
    let at = UNIX_EPOCH + Duration::from_millis(now_ms);
    humantime::format_rfc3339_seconds(at)
        .to_string()
        .replace(':', "")
}

/// Makes `plan.backup` and moves `.txtodo`, then the guard in, then every document. On failure it
/// moves back what it moved ([`move_back`]) and says what went wrong, and what stayed if any did.
pub(crate) fn move_aside(plan: &Plan) -> Result<(), String> {
    // `create_dir`, not `create_dir_all`: a folder that is there already is not ours to fill.
    // Ref: https://doc.rust-lang.org/std/fs/fn.create_dir.html
    std::fs::create_dir(&plan.backup)
        .map_err(|e| format!("create {}: {e}", plan.backup.display()))?;
    let mut moved = Vec::new();
    let Err(e) = move_all(plan, &mut moved) else {
        return Ok(());
    };
    let back = move_back(plan, &moved);
    // The failed entry may have made its folders in the backup before its rename failed.
    remove_empty_dirs(plan, &plan.entries());
    match back {
        Ok(()) => Err(format!("{e}; nothing was moved in the end")),
        Err(stuck) => Err(format!("{e}; and moving back failed: {stuck}")),
    }
}

fn move_all(plan: &Plan, moved: &mut Vec<String>) -> Result<(), String> {
    if plan.state {
        move_one(&plan.root, &plan.backup, STATE_DIR)?;
        moved.push(STATE_DIR.to_owned());
    }
    write_guard(plan)?;
    for rel in &plan.documents {
        move_one(&plan.root, &plan.backup, rel)?;
        moved.push(rel.clone());
    }
    Ok(())
}

/// Moves `moved` back into the root, newest first, never over something that is there again: such
/// a path stays in the backup and is named in the error. The guard goes just before `.txtodo`
/// comes back, and by the end unless `.txtodo` stayed. The backup folder goes when it ends empty.
pub(crate) fn move_back(plan: &Plan, moved: &[String]) -> Result<(), String> {
    let (mut stayed, mut guarded) = (Vec::new(), false);
    for rel in moved.iter().rev() {
        let state = rel == STATE_DIR;
        if state {
            remove_guard(&plan.root)?;
        }
        if let Err(e) = move_back_one(plan, rel) {
            if state {
                // No store beside lines that came back: keep the folder closed to opens.
                write_guard(plan)?;
                guarded = true;
            }
            stayed.push(format!("{rel} ({e})"));
        }
    }
    if !guarded {
        remove_guard(&plan.root)?;
    }
    remove_empty_dirs(plan, moved);
    if stayed.is_empty() {
        return Ok(());
    }
    let backup = plan.backup.display();
    Err(format!("these stayed in {backup}: {}", stayed.join(", ")))
}

/// Removes the guard [`move_aside`] left, once the root is ready to open again. A missing guard
/// is fine; anything else named `.txtodo` is left alone.
pub(crate) fn remove_guard(root: &Path) -> Result<(), String> {
    let path = root.join(STATE_DIR);
    let is_guard = std::fs::read_to_string(&path).is_ok_and(|text| text.starts_with(GUARD_HEAD));
    if !is_guard {
        return Ok(());
    }
    std::fs::remove_file(&path).map_err(|e| format!("remove {}: {e}", path.display()))
}

/// One rename, making the destination's folders first. The backup sits beside the root, so this
/// stays on one filesystem: a rename, never a copy.
/// Ref: https://doc.rust-lang.org/std/fs/fn.rename.html
fn move_one(from_root: &Path, to_root: &Path, rel: &str) -> Result<(), String> {
    let (from, to) = (from_root.join(rel), to_root.join(rel));
    if let Some(dir) = to.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("create {}: {e}", dir.display()))?;
    }
    std::fs::rename(&from, &to)
        .map_err(|e| format!("move {} to {}: {e}", from.display(), to.display()))
}

fn move_back_one(plan: &Plan, rel: &str) -> Result<(), String> {
    if std::fs::symlink_metadata(plan.root.join(rel)).is_ok() {
        return Err("something new is there".to_owned());
    }
    move_one(&plan.backup, &plan.root, rel)
}

/// The guard: a plain file where `.txtodo/` was, saying where the copy went.
fn write_guard(plan: &Plan) -> Result<(), String> {
    let path = plan.root.join(STATE_DIR);
    // `create_new`: the name must be free, since `.txtodo` just moved out.
    // Ref: https://doc.rust-lang.org/std/fs/struct.OpenOptions.html#method.create_new
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|e| format!("create {}: {e}", path.display()))?;
    let text = format!(
        "{GUARD_HEAD}: this folder's copy is moving to {}.\nIf this file is still here, the \
         move stopped half way; everything that moved is in that folder.\n",
        plan.backup.display()
    );
    file.write_all(text.as_bytes())
        .map_err(|e| format!("write {}: {e}", path.display()))
}

/// Removes the folders [`move_one`] made in the backup, deepest first, then the backup itself.
/// `remove_dir` only ever removes an empty folder, so anything that stayed keeps its folder.
/// Ref: https://doc.rust-lang.org/std/fs/fn.remove_dir.html
fn remove_empty_dirs(plan: &Plan, moved: &[String]) {
    let mut dirs: Vec<PathBuf> = moved
        .iter()
        .flat_map(|rel| Path::new(rel).ancestors().skip(1))
        .filter(|dir| !dir.as_os_str().is_empty())
        .map(|dir| plan.backup.join(dir))
        .collect();
    dirs.sort_by_key(|dir| std::cmp::Reverse(dir.components().count()));
    dirs.dedup();
    for dir in dirs.iter().chain(std::iter::once(&plan.backup)) {
        let _ = std::fs::remove_dir(dir);
    }
}

#[cfg(test)]
#[path = "rejoin_backup_tests.rs"]
mod tests;
