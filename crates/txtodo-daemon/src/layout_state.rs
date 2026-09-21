//! A workspace's live layout (task `workspace-layout`): where its root list and the `ref:`
//! directories of that list's lines live. One handle is shared by the `Workspace` and every
//! `FileActor` in it, so a layout read from `txtodo.toml` reaches all of them at once.

use std::sync::{Arc, PoisonError, RwLock};
use txtodo_model::WorkspaceLayout;

/// A cheap-to-clone handle on the workspace's current layout.
#[derive(Clone, Debug)]
pub struct SharedLayout(Arc<RwLock<WorkspaceLayout>>);

impl SharedLayout {
    /// A handle holding `layout`.
    pub fn new(layout: WorkspaceLayout) -> SharedLayout {
        SharedLayout(Arc::new(RwLock::new(layout)))
    }

    /// The current layout, by value: a caller never holds the lock across an await.
    pub fn get(&self) -> WorkspaceLayout {
        self.0
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// Replaces the layout for everything sharing this handle.
    pub fn set(&self, layout: WorkspaceLayout) {
        *self.0.write().unwrap_or_else(PoisonError::into_inner) = layout;
    }
}

impl Default for SharedLayout {
    /// ADR 0012's placement, beside the list, until a layout is loaded from `txtodo.toml` and the
    /// default flips to `tasks/` (task workspace-layout).
    fn default() -> SharedLayout {
        SharedLayout::new(WorkspaceLayout::beside_the_list())
    }
}
