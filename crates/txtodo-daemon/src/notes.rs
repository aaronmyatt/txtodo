//! `notes.md` as a Loro text doc (plan M5, design §7): `GetNotes`/`EditNotes`. Owned end-to-end by
//! the desktop-detail-view backend task; the RPC delegates here from `server.rs` untouched.
//!
//! Both RPCs take a bare wire `TaskRef` (`line_number`, `task_id`) with **no path** — unlike every
//! other RPC's `TaskRef`, which always rides beside a sibling `path` field. So the daemon resolves
//! the task purely by id, asking each registered document in turn "do you hold this task"
//! (`locate_task`, built on `notes_lookup.rs`) rather than requiring the client to already know
//! which document it lives in — consistent with design §7's "the daemon resolves the task to its
//! ref dir, never the client". `line_number` on the request is therefore ignored; the freshly
//! resolved line is used instead wherever one is needed (`ensure_ref_dir`).

use crate::actor::hash_of;
use crate::convert::{parse_required_task_id, status_of};
use crate::handle::{ActorHandle, Applied};
use crate::mutation::TaskRef;
use crate::notes_lookup::TaskLineInfo;
use crate::server::TxtodoService;
use tonic::{Request, Response, Status};
use txtodo_model::{FilePath, Principal, TaskId};
use txtodo_proto::v1 as pb;

impl TxtodoService {
    /// Returns the current `notes.md` bytes for the task's `ref:` directory; empty (no `path`,
    /// empty bytes, the empty hash) when the task has no `ref:` directory yet.
    pub(crate) async fn get_notes_impl(
        &self,
        r: Request<pb::GetNotesRequest>,
    ) -> Result<Response<pb::NotesDoc>, Status> {
        let task = r
            .into_inner()
            .task
            .ok_or_else(|| Status::invalid_argument("a task ref is required"))?;
        let task_id = parse_required_task_id(&task)?;
        let Some((owner, info)) = self.locate_task(task_id).await? else {
            return Err(Status::not_found(format!("no task {task_id}")));
        };
        let Some(slug) = info.slug else {
            return Ok(Response::new(pb::NotesDoc {
                path: String::new(),
                bytes: Vec::new(),
                hash: hash_of(&[]).to_vec(),
            }));
        };
        let notes_path = ref_notes_path(&owner, &slug);
        let actor = self
            .workspace()
            .notes_actor(&notes_path)
            .map_err(status_of)?;
        let (bytes, hash) = {
            let actor = actor
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            actor.contents()
        };
        Ok(Response::new(pb::NotesDoc {
            path: notes_path.to_string(),
            bytes,
            hash: hash.to_vec(),
        }))
    }

    /// Applies one whole-document edit to `notes.md`, lazily creating the `ref:` directory and
    /// tag on the first edit when neither exists yet (plan §3.2.4): `ensure_ref_dir` lands first,
    /// as its own op batch on the owner document, and only on success does the notes write run —
    /// a directory-creation failure must not leave a notes edit applied against nothing.
    pub(crate) async fn edit_notes_impl(
        &self,
        r: Request<pb::NotesEditRequest>,
    ) -> Result<Response<pb::ApplyResponse>, Status> {
        let req = r.into_inner();
        let task = req
            .task
            .ok_or_else(|| Status::invalid_argument("edit_notes needs a task"))?;
        let task_id = parse_required_task_id(&task)?;
        let Some((owner, info)) = self.locate_task(task_id).await? else {
            return Err(Status::not_found(format!("no task {task_id}")));
        };
        let device = self.workspace().device();
        let principal = Principal::User { device };
        let slug = self
            .slug_for(&owner, task_id, info, principal.clone())
            .await?;
        let notes_path = ref_notes_path(&owner, &slug);
        let actor = self
            .workspace()
            .notes_actor(&notes_path)
            .map_err(status_of)?;
        let applied = {
            let mut actor = actor
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            actor.edit(&req.new_text, principal)
        }
        .map_err(status_of)?;
        Ok(Response::new(applied_of(applied)))
    }

    /// The task's `ref:` slug, creating the directory and tag first when it has none yet.
    async fn slug_for(
        &self,
        owner: &FilePath,
        task_id: TaskId,
        info: TaskLineInfo,
        principal: Principal,
    ) -> Result<String, Status> {
        if let Some(slug) = info.slug {
            return Ok(slug);
        }
        let owner_handle = self.actor_by_path(owner)?;
        let task_ref = TaskRef {
            line_number: info.line_number,
            task_id: Some(task_id),
        };
        let created = owner_handle
            .ensure_ref_dir(task_ref, principal)
            .await
            .map_err(status_of)?;
        Ok(created.slug)
    }

    /// The document holding `task_id`, and its current line/slug, searching every registered
    /// document (see the module doc for why a path cannot be read off the request itself).
    async fn locate_task(
        &self,
        task_id: TaskId,
    ) -> Result<Option<(FilePath, TaskLineInfo)>, Status> {
        let handles: Vec<ActorHandle> = {
            let ws = self.workspace();
            ws.paths().filter_map(|p| ws.actor(p).cloned()).collect()
        };
        for h in handles {
            if let Some(info) = h.task_line(task_id).await.map_err(status_of)? {
                return Ok(Some((h.path().clone(), info)));
            }
        }
        Ok(None)
    }
}

/// `<owner's directory>/<slug>/notes.md`, workspace-relative — the same directory
/// `refdir_ops.rs::own_dir` joins the slug onto, expressed on `FilePath` strings instead of
/// absolute paths since the notes actor is addressed by workspace-relative path like every other
/// document.
fn ref_notes_path(owner: &FilePath, slug: &str) -> FilePath {
    let p = owner.as_str();
    let joined = match p.rfind('/') {
        Some(i) => format!("{}/{slug}/notes.md", &p[..i]),
        None => format!("{slug}/notes.md"),
    };
    let path = FilePath::new(&joined);
    debug_assert!(
        path.is_ok(),
        "a slug beside a valid path is valid: {joined:?}"
    );
    path.unwrap_or_else(|_| owner.clone())
}

fn applied_of(a: Applied) -> pb::ApplyResponse {
    pb::ApplyResponse {
        applied: a.applied,
        hash: a.hash.to_vec(),
        hlc_wall_ms: a.hlc.wall_ms,
        hlc_counter: u32::from(a.hlc.counter),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ref_notes_path_joins_beside_the_owner_and_at_the_root() {
        let nested = FilePath::new("q4/todo.txt").unwrap();
        assert_eq!(
            ref_notes_path(&nested, "buy-ducks").as_str(),
            "q4/buy-ducks/notes.md"
        );
        let root = FilePath::new("todo.txt").unwrap();
        assert_eq!(
            ref_notes_path(&root, "buy-ducks").as_str(),
            "buy-ducks/notes.md"
        );
    }
}
