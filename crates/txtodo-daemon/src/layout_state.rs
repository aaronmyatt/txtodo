//! A workspace's live layout (task `workspace-layout`): where its root list and the `ref:`
//! directories of that list's lines live. One handle is shared by the `Workspace` and every
//! `FileActor` in it, so a layout read from `txtodo.toml` reaches all of them at once.

use std::sync::{Arc, Mutex, PoisonError, RwLock};
use txtodo_model::WorkspaceLayout;

#[derive(Debug)]
struct Inner {
    layout: RwLock<WorkspaceLayout>,
    /// Why the file's layout is not the one in force (a bad file, or a change refused while ref
    /// dirs still sit in the old place); `None` when the file and the layout agree.
    note: Mutex<Option<String>>,
}

/// A cheap-to-clone handle on the workspace's current layout.
#[derive(Clone, Debug)]
pub struct SharedLayout(Arc<Inner>);

impl SharedLayout {
    /// A handle holding `layout`.
    pub fn new(layout: WorkspaceLayout) -> SharedLayout {
        SharedLayout(Arc::new(Inner {
            layout: RwLock::new(layout),
            note: Mutex::new(None),
        }))
    }

    /// The current layout, by value: a caller never holds the lock across an await.
    pub fn get(&self) -> WorkspaceLayout {
        self.0
            .layout
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// Replaces the layout for everything sharing this handle.
    pub fn set(&self, layout: WorkspaceLayout) {
        *self
            .0
            .layout
            .write()
            .unwrap_or_else(PoisonError::into_inner) = layout;
    }

    /// Why the layout file is not in force, if it is not.
    pub fn note(&self) -> Option<String> {
        self.0
            .note
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// Records (or clears, with `None`) why the layout file is not in force.
    pub fn set_note(&self, note: Option<String>) {
        *self.0.note.lock().unwrap_or_else(PoisonError::into_inner) = note;
    }
}

impl Default for SharedLayout {
    /// ADR 0012's placement, beside the list. Only hand-built test actors use this; a `Workspace`
    /// starts from `layout_file::initial`, whose default is `tasks/`.
    fn default() -> SharedLayout {
        SharedLayout::new(WorkspaceLayout::beside_the_list())
    }
}
