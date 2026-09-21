//! Hot reload of `<root>/txtodo.toml` (task `workspace-layout`): the watcher routes a change to that
//! file here. A valid new layout takes effect for every actor at once, unless ref dirs still sit
//! in the old place, in which case it is refused (moving them is an explicit act). A bad file, or
//! one that was deleted, keeps the last good layout. Either way the reason is left on the shared
//! layout for `doctor` to show.

use crate::layout_file::{self, LAYOUT_FILE, LayoutFile};
use crate::layout_state::SharedLayout;
use crate::server::SharedWorkspace;
use std::path::Path;
use txtodo_model::WorkspaceLayout;

/// True when `path` is the workspace's layout file.
pub(crate) fn is_layout_file(root: &Path, path: &Path) -> bool {
    path == root.join(LAYOUT_FILE)
}

/// Re-reads the layout file and applies it if it may be applied.
pub(crate) async fn reload_layout(ws: &SharedWorkspace) {
    let (root, shared, root_actor) = {
        let guard = ws.read().unwrap_or_else(std::sync::PoisonError::into_inner);
        let shared = guard.layout().clone();
        let root_list = shared.get().root_list();
        (
            guard.root().to_path_buf(),
            shared,
            guard.actor(&root_list).cloned(),
        )
    };
    match layout_file::read(&root) {
        LayoutFile::Missing => shared.set_note(None), // deleted: keep the last good layout
        LayoutFile::Invalid(why) => {
            log_kept(&why);
            shared.set_note(Some(format!("{why}; keeping the last good layout")));
        }
        LayoutFile::Valid(new) => {
            let in_use = match root_actor {
                Some(actor) => refs_in_the_old_place(&actor, &root, &shared.get()).await,
                None => 0,
            };
            apply(ws, &shared, new, in_use);
        }
    }
}

/// How many of the root list's `ref:` dirs exist where the current layout puts them.
async fn refs_in_the_old_place(
    actor: &crate::handle::ActorHandle,
    root: &Path,
    current: &WorkspaceLayout,
) -> usize {
    let Ok(tags) = actor.ref_tags().await else {
        return 0;
    };
    let list = current.root_list();
    tags.iter()
        .filter(|t| root.join(current.ref_dir_for(&list, &t.slug)).exists())
        .count()
}

fn apply(ws: &SharedWorkspace, shared: &SharedLayout, new: WorkspaceLayout, in_use: usize) {
    if new == shared.get() {
        shared.set_note(None);
    } else if in_use > 0 {
        let why = format!(
            "layout change refused: {in_use} ref dir(s) still sit where the current layout puts them; move them first"
        );
        log_kept(&why);
        shared.set_note(Some(why));
    } else {
        shared.set(new);
        shared.set_note(None);
        let guard = ws.read().unwrap_or_else(std::sync::PoisonError::into_inner);
        guard.tree_dirty.mark();
        log_applied();
    }
}

fn log_kept(why: &str) {
    tracing::warn!(why, "layout_reload_kept_last_good");
}

fn log_applied() {
    tracing::info!("layout_reloaded");
}
