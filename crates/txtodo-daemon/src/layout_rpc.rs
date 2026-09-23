//! The `WorkspaceLayout` RPC (task `workspace-layout`): reads a workspace's layout, or changes it.
//! A change is refused while ref dirs sit where the current layout puts them, unless the caller
//! asks to move them; either way it is written to `<root>/txtodo.toml`, so it survives a restart and
//! travels with the workspace. The hot reload in `layout_reload.rs` sees the same file and finds the
//! layout already in force.

use crate::layout_file::LAYOUT_FILE;
use crate::server::TxtodoService;
use std::path::{Path, PathBuf};
use tonic::{Request, Response, Status};
use txtodo_model::{FilePath, WorkspaceLayout};
use txtodo_proto::v1 as pb;

impl TxtodoService {
    pub(crate) async fn workspace_layout_impl(
        &self,
        r: Request<pb::WorkspaceLayoutRequest>,
    ) -> Result<Response<pb::WorkspaceLayoutInfo>, Status> {
        let req = r.into_inner();
        let (root, shared, root_actor) = {
            let ws = self.workspace();
            let shared = ws.layout().clone();
            let root_list = shared.get().root_list();
            (
                ws.root().to_path_buf(),
                shared,
                ws.actor(&root_list).cloned(),
            )
        };
        let current = shared.get();
        let mut moved = 0;
        if req.set {
            let new = requested(&current, &req)?;
            if new != current {
                let tags = match &root_actor {
                    Some(actor) => actor.ref_tags().await.map_err(crate::convert::status_of)?,
                    None => Vec::new(),
                };
                let slugs: Vec<&str> = tags.iter().map(|t| t.slug.as_str()).collect();
                let list_changed = new.todo_file() != current.todo_file();
                if list_changed {
                    // Before the file and the switch: a root list that cannot be made refuses
                    // the change instead of leaving a layout with no list (layout-reload-safety).
                    crate::layout_reload::create_root_list_file(&root, &new)
                        .map_err(|e| Status::failed_precondition(e.to_string()))?;
                }
                moved = relocate(&root, (&current, &new), &slugs, req.move_dirs)?;
                write_layout_file(&root, &new)?;
                // The new bytes go to paired devices as an op (`layout_sync.rs`).
                let ws = self.workspace();
                let device = ws.device();
                crate::layout_sync::record_disk(&ws, txtodo_model::Principal::User { device });
                drop(ws);
                shared.set(new);
                shared.set_note(None);
                if list_changed {
                    crate::layout_reload::register_root_list(&self.ws)
                        .map_err(|e| Status::internal(e.to_string()))?;
                }
                self.workspace().tree_dirty.mark();
            }
        }
        let layout = shared.get();
        let outside = self.outside_refs_dir(&layout).await?;
        Ok(Response::new(pb::WorkspaceLayoutInfo {
            refs_dir: layout.refs_dir().to_owned(),
            todo_file: layout.todo_file().to_owned(),
            note: shared.note().unwrap_or_default(),
            moved,
            outside_refs_dir: outside,
        }))
    }

    /// Directories nothing points at that sit outside `refs_dir`: ref dirs an older layout left
    /// behind, which `prune` will not touch.
    async fn outside_refs_dir(&self, layout: &WorkspaceLayout) -> Result<Vec<String>, Status> {
        let tree = self.workspace_tree().await?;
        let mut dirs: Vec<String> = tree
            .orphans()
            .filter_map(|id| id.as_dir().map(ToString::to_string))
            .filter(|d| !crate::refdir_grpc::is_prune_candidate(layout, d))
            .collect();
        dirs.sort();
        Ok(dirs)
    }
}

/// The layout the request asks for: its fields over the current one, validated.
fn requested(
    current: &WorkspaceLayout,
    req: &pb::WorkspaceLayoutRequest,
) -> Result<WorkspaceLayout, Status> {
    let pick = |asked: &str, kept: &str| {
        if asked.is_empty() {
            kept.to_owned()
        } else {
            asked.to_owned()
        }
    };
    let refs_dir = pick(&req.refs_dir, current.refs_dir());
    let todo_file = pick(&req.todo_file, current.todo_file());
    WorkspaceLayout::new(&refs_dir, &todo_file).map_err(|e| Status::invalid_argument(e.to_string()))
}

/// Moves each of `slugs`' directories from where `old` puts them to where `new` will, when they
/// exist. Refused, moving nothing, when any exist and `move_dirs` is false, or when a target is
/// already taken. On a failure part way, what was moved is put back. Returns how many moved.
fn relocate(
    root: &Path,
    (old, new): (&WorkspaceLayout, &WorkspaceLayout),
    slugs: &[&str],
    move_dirs: bool,
) -> Result<u32, Status> {
    let list: FilePath = old.root_list();
    let pairs: Vec<(PathBuf, PathBuf)> = slugs
        .iter()
        .map(|s| {
            (
                root.join(old.ref_dir_for(&list, s)),
                root.join(new.ref_dir_for(&list, s)),
            )
        })
        .filter(|(from, _)| from.exists())
        .collect();
    if pairs.is_empty() {
        return Ok(0);
    }
    if !move_dirs {
        return Err(Status::failed_precondition(format!(
            "{} ref dir(s) sit where the current layout puts them; ask to move them, or move them first",
            pairs.len()
        )));
    }
    if let Some((_, to)) = pairs.iter().find(|(_, to)| to.exists()) {
        return Err(Status::failed_precondition(format!(
            "{} is already there; nothing was moved",
            to.strip_prefix(root).unwrap_or(to).display()
        )));
    }
    let mut done: Vec<&(PathBuf, PathBuf)> = Vec::new();
    for pair in &pairs {
        if let Err(e) = move_one(pair) {
            for (from, to) in done.into_iter().rev() {
                let _ = std::fs::rename(to, from);
            }
            return Err(Status::internal(format!(
                "cannot move {}: {e}",
                pair.0.display()
            )));
        }
        done.push(pair);
    }
    Ok(u32::try_from(pairs.len()).unwrap_or(u32::MAX))
}

fn move_one((from, to): &(PathBuf, PathBuf)) -> std::io::Result<()> {
    if let Some(parent) = to.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::rename(from, to)
}

/// Writes `<root>/txtodo.toml` through a temp file and a rename, so a reader never sees half of it.
/// The temp name starts with `.txtodo-`, which the watcher ignores.
fn write_layout_file(root: &Path, layout: &WorkspaceLayout) -> Result<(), Status> {
    let text = format!(
        "# Where this workspace keeps its root list and the folder for its ref: lines.\n\
         refs_dir = {:?}\ntodo_file = {:?}\n",
        layout.refs_dir(),
        layout.todo_file()
    );
    let tmp = root.join(".txtodo-layout.tmp");
    std::fs::write(&tmp, text)
        .and_then(|()| std::fs::rename(&tmp, root.join(LAYOUT_FILE)))
        .map_err(|e| Status::internal(format!("cannot write {LAYOUT_FILE}: {e}")))
}
