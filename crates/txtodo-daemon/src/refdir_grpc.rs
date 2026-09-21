//! `RefDir`/`PruneOrphans` gRPC handlers (plan M5, specs/ref-directories.md rules 2, 4, 9, 10):
//! thin wrappers over the already-implemented `daemon-ref-creation` actor methods
//! (`ActorHandle::resolve_ref_dir`/`ensure_ref_dir`, `refdir.rs`/`refdir_ops.rs`) and the cached
//! workspace tree (`tree.rs`). `server.rs`'s `ref_dir`/`prune_orphans` trait methods delegate here
//! untouched, same pattern as `notes.rs`.

use crate::convert::{parse_path, parse_task_ref, status_of};
use crate::server::TxtodoService;
use tonic::{Request, Response, Status};
use txtodo_model::{FilePath, Principal, WorkspaceLayout};
use txtodo_proto::v1 as pb;

/// `<owner's directory>/<slug>`, workspace-relative — the same directory `refdir_ops.rs::own_dir`
/// joins the slug onto, expressed on `FilePath` strings like `notes.rs::ref_notes_path` (which adds
/// `/notes.md` on top of exactly this).
fn ref_dir_path(layout: &WorkspaceLayout, owner: &FilePath, slug: &str) -> FilePath {
    FilePath::new(&layout.ref_dir_for(owner, slug)).unwrap_or_else(|_| owner.clone())
}

/// Whether `dir` may be offered to `prune`: with refs under `refs_dir`, only what sits inside it,
/// so `crates/` and `docs/` beside the list are never orphans (task workspace-layout). With refs
/// beside the list (`.`) every directory is a candidate, as before.
fn is_prune_candidate(layout: &WorkspaceLayout, dir: &str) -> bool {
    layout.refs_beside_list() || dir.starts_with(&format!("{}/", layout.refs_dir()))
}

impl TxtodoService {
    /// Resolves (`ensure = false`) or lazily creates (`ensure = true`) one line's `ref:`
    /// directory. Never writes anything when `ensure` is false — `txtodo open`'s negative-space
    /// requirement.
    pub(crate) async fn ref_dir_impl(
        &self,
        r: Request<pb::RefDirRequest>,
    ) -> Result<Response<pb::RefDirInfo>, Status> {
        let req = r.into_inner();
        let path = parse_path(&req.path)?;
        let task = parse_task_ref(req.task)?;
        let handle = self.actor_by_path(&path)?;
        let query = handle
            .resolve_ref_dir(task.clone())
            .await
            .map_err(status_of)?;
        if !req.ensure || query.has_ref_tag {
            let dir = ref_dir_path(&self.workspace().layout().get(), &path, &query.slug);
            let dir_exists = self.workspace().root().join(dir.as_str()).exists();
            return Ok(Response::new(pb::RefDirInfo {
                task_id: query.task_id.to_string(),
                slug: query.slug,
                dir: dir.to_string(),
                has_ref_tag: query.has_ref_tag,
                dir_exists,
            }));
        }
        let device = self.workspace().device();
        let created = handle
            .ensure_ref_dir(task, Principal::User { device })
            .await
            .map_err(status_of)?;
        Ok(Response::new(pb::RefDirInfo {
            task_id: query.task_id.to_string(),
            dir: ref_dir_path(&self.workspace().layout().get(), &path, &created.slug).to_string(),
            slug: created.slug,
            has_ref_tag: true,
            dir_exists: true,
        }))
    }

    /// Lists `ref:` directories no line points to (rule 10); deletes them only when `execute` is
    /// set. See the module doc's known gap on a directory deleted out from under a still-live
    /// actor (no actor-teardown API exists yet — `Workspace` has none, and building one is a
    /// larger change than this task).
    pub(crate) async fn prune_orphans_impl(
        &self,
        r: Request<pb::PruneOrphansRequest>,
    ) -> Result<Response<pb::PruneOrphansResponse>, Status> {
        let execute = r.into_inner().execute;
        let tree = self.workspace_tree().await?;
        let layout = self.workspace().layout().get();
        let mut dirs: Vec<String> = tree
            .orphans()
            .filter_map(|id| id.as_dir().map(ToString::to_string))
            .filter(|d| is_prune_candidate(&layout, d))
            .collect();
        dirs.sort();
        if execute {
            self.delete_orphans(&dirs)?;
        }
        Ok(Response::new(pb::PruneOrphansResponse {
            dirs,
            executed: execute,
        }))
    }

    fn delete_orphans(&self, dirs: &[String]) -> Result<(), Status> {
        let root = self.workspace().root().to_path_buf();
        for d in dirs {
            let abs = root.join(d);
            std::fs::remove_dir_all(&abs)
                .map_err(|e| Status::internal(format!("cannot remove {d}: {e}")))?;
        }
        self.workspace().tree_dirty.mark();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ref_dir_path_joins_beside_the_owner_and_by_the_layout_at_the_root() {
        let beside = WorkspaceLayout::beside_the_list();
        let nested = FilePath::new("q4/todo.txt").unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(
            ref_dir_path(&beside, &nested, "buy-ducks").as_str(),
            "q4/buy-ducks"
        );
        let root = FilePath::new("todo.txt").unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(
            ref_dir_path(&beside, &root, "buy-ducks").as_str(),
            "buy-ducks"
        );
        let tasks = WorkspaceLayout::default();
        assert_eq!(
            ref_dir_path(&tasks, &root, "buy-ducks").as_str(),
            "tasks/buy-ducks"
        );
    }
}
