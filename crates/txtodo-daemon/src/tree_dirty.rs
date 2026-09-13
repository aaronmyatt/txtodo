//! Whether `Workspace`'s cached [`txtodo_model::WorkspaceTree`] might be stale after a commit
//! (plan M5, tasks/model-workspace-tree/notes.md). Every `FileActor` shares one of these (see
//! `ActorConfig::tree_dirty`) and marks it after a commit whose ops `txtodo_model::invalidates`
//! says could change the tree; `tree.rs`'s `TxtodoService::workspace_tree` rebuilds from live actor
//! state only when it is set, and never for an op `invalidates` says cannot touch the tree at all
//! (a `NotesEdit`, a bare priority change, ...) — see that module for why this is not "the cheap
//! wrong version" of recomputing the whole workspace on every op.

use std::sync::atomic::{AtomicBool, Ordering};

/// Shared by every actor in one workspace and its cached tree.
#[derive(Debug)]
pub struct TreeDirty(AtomicBool);

impl Default for TreeDirty {
    /// Starts dirty: nothing has been cached yet, so the first read must build it.
    fn default() -> TreeDirty {
        TreeDirty(AtomicBool::new(true))
    }
}

impl TreeDirty {
    /// Marks the cache stale.
    pub fn mark(&self) {
        self.0.store(true, Ordering::Relaxed);
    }

    /// True since the last [`TreeDirty::clear`] (or construction).
    pub fn is_dirty(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }

    /// Clears the flag, after a rebuild has caught up with everything marked so far. A commit that
    /// lands *during* a rebuild and marks the flag again is not lost: `clear` only ever races
    /// towards "dirty", never away from it, from this thread's point of view — see the module doc's
    /// callers, which always rebuild before clearing.
    pub fn clear(&self) {
        self.0.store(false, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starts_dirty_and_tracks_mark_and_clear() {
        let d = TreeDirty::default();
        assert!(d.is_dirty(), "nothing cached yet");
        d.clear();
        assert!(!d.is_dirty());
        d.mark();
        assert!(d.is_dirty());
    }
}
