//! Discovery half of `Workspace` (plan §3.2.11), split from `workspace.rs` for its file budget:
//! walk a directory and register every document found. The watcher walks first with no lock held
//! and calls only [`Workspace::register_discovered`] under the write lock.

use crate::walker;
use crate::workspace::Workspace;
use crate::workspace_error::WorkspaceError;
use std::path::Path;
use txtodo_model::FilePath;

impl Workspace {
    /// Walks `dir` (the root or a newly created subdirectory) and registers every document found.
    /// Returns how many actors were started.
    pub fn discover(&mut self, dir: &Path) -> Result<usize, WorkspaceError> {
        let extra = self.extra_document();
        self.register_discovered(dir, walker::walk_with(dir, extra.as_deref())?)
    }

    /// The root list's absolute path when its name is not one the walker finds on its own
    /// (`todo_file` in `txtodo.toml`, task workspace-layout); `None` for `todo.txt`.
    pub fn extra_document(&self) -> Option<std::path::PathBuf> {
        self.layout()
            .get()
            .custom_root_list()
            .map(|p| self.root().join(p.as_str()))
    }

    /// The registering half of [`Self::discover`], for a caller that already walked `dir` — the
    /// watcher walks first with no lock held and takes the workspace write lock only for this, so
    /// a slow walk never starves readers (root todo id:01M2WK7W1MPDW9VBWS25EF8CB5).
    pub fn register_discovered(
        &mut self,
        dir: &Path,
        found: Vec<FilePath>,
    ) -> Result<usize, WorkspaceError> {
        debug_assert!(
            dir.starts_with(self.root()),
            "discover stays inside the workspace"
        );
        let mut started = 0usize;
        for rel in found {
            let abs = dir.join(rel.as_str());
            let path = walker::relative(self.root(), &abs)?;
            if self.register(path)? {
                started += 1;
            }
        }
        Ok(started)
    }
}
