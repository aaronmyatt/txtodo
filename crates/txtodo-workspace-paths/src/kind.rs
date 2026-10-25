//! Which carrier syncs a workspace: txtodo, or git (task `two-kinds-of-workspace`, ADR 0043).
//!
//! The kind is read off where the folder is, never stored, so no setting can say "both":
//!
//! - **List**: the default workspace's folder, or a folder inside [`lists_dir_for`]. txtodo syncs
//!   it between the user's own devices.
//! - **Repo**: every other folder (a git checkout, a plain folder, an old `remote/<id>/` mirror).
//!   txtodo never syncs it; git does.

use std::path::{Path, PathBuf};

use crate::{RegistryEnv, default_workspace_dir_for, global_state_dir};

/// Where named lists live (task `two-kinds-of-workspace`): `lists` beside the default workspace
/// and `remote`, one `<name>/` folder each. Relocated with an isolated daemon's socket, the same
/// way [`default_workspace_dir_for`] is, so a test daemon never writes the real data dir. The
/// daemon owns this folder: anything registered under it is a list.
pub fn lists_dir_for(env: &RegistryEnv) -> PathBuf {
    global_state_dir(env).join("lists")
}

/// Which carrier syncs a workspace (ADR 0043).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum WorkspaceKind {
    /// txtodo syncs it between the user's own devices.
    List,
    /// git syncs it; txtodo does not.
    Repo,
}

impl WorkspaceKind {
    /// `"list"` or `"repo"`, as a client prints it.
    pub fn as_str(self) -> &'static str {
        match self {
            WorkspaceKind::List => "list",
            WorkspaceKind::Repo => "repo",
        }
    }
}

/// The kind of the workspace rooted at `root`: [`WorkspaceKind::List`] for the default
/// workspace's folder and for a folder inside the lists directory, else [`WorkspaceKind::Repo`].
///
/// `root` and the env's folders are compared as given and canonicalized, because the registry
/// stores canonical roots (macOS's `/var` is `/private/var`) while the env holds whatever path was
/// configured, and the lists directory does not exist until the first list is made.
pub fn workspace_kind(env: &RegistryEnv, root: &Path) -> WorkspaceKind {
    let default = default_workspace_dir_for(env);
    let lists = lists_dir_for(env);
    if same_folder(root, &default) || inside(root, &lists) {
        WorkspaceKind::List
    } else {
        WorkspaceKind::Repo
    }
}

/// True when `a` and `b` are the same folder, as given or once canonical.
fn same_folder(a: &Path, b: &Path) -> bool {
    a == b || canonical_lenient(a) == canonical_lenient(b)
}

/// True when `path` is a folder inside `dir` (not `dir` itself), as given or once canonical.
fn inside(path: &Path, dir: &Path) -> bool {
    let under = |p: &Path, d: &Path| p != d && p.starts_with(d);
    under(path, dir) || under(&canonical_lenient(path), &canonical_lenient(dir))
}

/// `path` canonicalized as far as it exists: the longest existing ancestor is resolved and the
/// rest is appended unchanged, so a folder that is not made yet still compares equal to its
/// canonical twin. `Path::canonicalize` fails on a missing path.
/// Ref: <https://doc.rust-lang.org/std/fs/fn.canonicalize.html>
fn canonical_lenient(path: &Path) -> PathBuf {
    let mut rest = Vec::new();
    let mut here = path;
    loop {
        if let Ok(real) = here.canonicalize() {
            return rest.iter().rev().fold(real, |acc, part| acc.join(part));
        }
        match (here.parent(), here.file_name()) {
            (Some(parent), Some(name)) => {
                rest.push(name);
                here = parent;
            }
            _ => return path.to_path_buf(),
        }
    }
}

#[cfg(test)]
#[path = "kind_tests.rs"]
mod tests;
