//! Hot reload of `<root>/txtodo.toml` (task `workspace-layout`): the watcher routes a change to that
//! file here. A valid new layout takes effect for every actor at once, unless ref dirs still sit
//! in the old place, in which case it is refused (moving them is an explicit act). A bad file, or
//! one that was deleted, keeps the last good layout. Either way the reason is left on the shared
//! layout for `doctor` to show. Every failure here fails *closed* (task layout-reload-safety): an
//! unreadable root list refuses the change rather than assuming no ref dir is in the way, and a
//! root list that cannot be made or registered is reported, never shrugged off.

use crate::layout_file::{self, LAYOUT_FILE, LayoutFile};
use crate::layout_state::SharedLayout;
use crate::server::SharedWorkspace;
use std::fmt;
use std::path::{Path, PathBuf};
use txtodo_model::WorkspaceLayout;

/// Why the layout's root list could not be made a real, watched document.
#[derive(Debug)]
pub(crate) enum RootListError {
    /// Creating the file (or its folder) failed.
    Io {
        /// What was being made.
        path: PathBuf,
        /// The OS error.
        source: std::io::Error,
    },
    /// The daemon could not start an actor for it.
    Register(crate::workspace_error::WorkspaceError),
}

impl fmt::Display for RootListError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RootListError::Io { path, source } => {
                write!(
                    f,
                    "cannot create the root list {}: {source}",
                    path.display()
                )
            }
            RootListError::Register(e) => write!(f, "cannot open the root list: {e}"),
        }
    }
}

impl std::error::Error for RootListError {}

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
        // Deleted: keep the last good layout, and say so when the next start would not agree
        // with it (`layout_file::initial` falls back to the defaults on a missing file).
        LayoutFile::Missing => shared.set_note(missing_file_note(&shared.get())),
        LayoutFile::Invalid(why) => {
            log_kept(&why);
            shared.set_note(Some(format!("{why}; keeping the last good layout")));
        }
        LayoutFile::Valid(new) => {
            let in_use = match root_actor {
                Some(actor) => refs_in_the_old_place(&actor, &root, &shared.get()).await,
                None => Ok(0),
            };
            match in_use {
                Ok(in_use) => apply(ws, &shared, new, in_use),
                Err(e) => {
                    // Fail closed: with the root list unreadable, "no ref dir is in the way"
                    // is a guess, and applying on a guess would orphan every sub-backlog.
                    let why = format!(
                        "layout change not applied: the root list could not be read ({e}); \
                         keeping the last good layout"
                    );
                    log_kept(&why);
                    shared.set_note(Some(why));
                }
            }
        }
    }
}

/// The note for a deleted `txtodo.toml`: nothing while the layout in force is the default the
/// next start would pick anyway; otherwise a warning that the workspace changes shape on restart.
fn missing_file_note(in_force: &WorkspaceLayout) -> Option<String> {
    (*in_force != WorkspaceLayout::default()).then(|| {
        format!(
            "{LAYOUT_FILE} was deleted; refs_dir = {} and todo_file = {} stay in force for this \
             run, but the next start uses the defaults (tasks, todo.txt) — restore the file or \
             move the ref dirs",
            in_force.refs_dir(),
            in_force.todo_file()
        )
    })
}

/// How many of the root list's `ref:` dirs exist where the current layout puts them. `Err` when
/// the root list cannot answer (mailbox full, actor gone, store error): the caller refuses the
/// change rather than reading that as zero.
async fn refs_in_the_old_place(
    actor: &crate::handle::ActorHandle,
    root: &Path,
    current: &WorkspaceLayout,
) -> Result<usize, crate::handle::ActorError> {
    let tags = actor.ref_tags().await?;
    let list = current.root_list();
    Ok(tags
        .iter()
        .filter(|t| root.join(current.ref_dir_for(&list, &t.slug)).exists())
        .count())
}

/// Creates the root list `layout` names, empty, when it is missing (its folder too). The RPC
/// path calls this *before* writing the layout file, so a name that cannot be made (an existing
/// directory, a denied write) refuses the change instead of leaving a layout with no list.
pub(crate) fn create_root_list_file(
    root: &Path,
    layout: &WorkspaceLayout,
) -> Result<(), RootListError> {
    let abs = root.join(layout.root_list().as_str());
    if abs.is_file() {
        return Ok(());
    }
    if let Some(parent) = abs.parent() {
        std::fs::create_dir_all(parent).map_err(|source| RootListError::Io {
            path: parent.to_path_buf(),
            source,
        })?;
    }
    std::fs::write(&abs, b"").map_err(|source| RootListError::Io { path: abs, source })
}

/// Makes the root list the layout names a real, watched document: creates it empty when missing
/// (its folder too) and starts its actor. Needed when `todo_file` changes on a live workspace, since
/// the walker only finds a custom name once it has been told the name. The RPC propagates a
/// failure to its caller; the watcher path logs it and leaves a note.
pub(crate) fn register_root_list(ws: &SharedWorkspace) -> Result<(), RootListError> {
    let (path, root, layout) = {
        let guard = ws.read().unwrap_or_else(std::sync::PoisonError::into_inner);
        let layout = guard.layout().get();
        (layout.root_list(), guard.root().to_path_buf(), layout)
    };
    create_root_list_file(&root, &layout)?;
    let mut guard = ws
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    guard
        .register(path)
        .map(|_| ())
        .map_err(RootListError::Register)
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
        let list_changed = new.todo_file() != shared.get().todo_file();
        shared.set(new);
        shared.set_note(None);
        if list_changed && let Err(e) = register_root_list(ws) {
            let why = format!("{e}; the layout is in force but its root list is not open");
            log_kept(&why);
            shared.set_note(Some(why));
        }
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
