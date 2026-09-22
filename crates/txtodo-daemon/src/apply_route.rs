//! Routes one `Apply` request's mutations (split out of `server.rs` for the file budget, same
//! `impl TxtodoService`). A batch that is exactly one cross-file `Move` is coordinated across two
//! actors and the task's `ref:` directory (`move_coordinator.rs`, plan §3.2.8); anything else goes
//! straight to the addressed document's own actor. A `Move` mixed with other mutations is refused
//! — this crate does not support that batch shape.

use crate::convert::status_of;
use crate::handle::{ActorError, Applied, Preview};
use crate::move_coordinator;
use crate::mutation::{Mutation, MutationError, TaskRef};
use crate::server::TxtodoService;
use std::path::Path;
use std::sync::PoisonError;
use tonic::Status;
use txtodo_model::{FilePath, Principal};

use crate::handle::ActorHandle;

impl TxtodoService {
    /// Applies `mutations` against the document at `path`.
    pub(crate) async fn route_apply(
        &self,
        path: FilePath,
        mutations: Vec<Mutation>,
        principal: Principal,
        source: Option<String>,
    ) -> Result<Applied, Status> {
        match <[Mutation; 1]>::try_from(mutations) {
            Ok([Mutation::Move { task, to }]) => {
                let origin = move_coordinator::Origin { principal, source };
                self.apply_move(&path, task, to, origin).await
            }
            Ok([other]) => self
                .actor_or_new_list(&path, std::slice::from_ref(&other))?
                .apply_from(vec![other], principal, source)
                .await
                .map_err(status_of),
            Err(mutations) if mutations.iter().any(|m| matches!(m, Mutation::Move { .. })) => {
                Err(status_of(ActorError::Mutation(MutationError::Unsupported(
                    "Move must be its own Apply batch",
                ))))
            }
            Err(mutations) => self
                .actor_or_new_list(&path, &mutations)?
                .apply_from(mutations, principal, source)
                .await
                .map_err(status_of),
        }
    }

    /// The addressed document's actor — registering a brand-new list first when every mutation
    /// is an `Add`, the file does not exist yet and its directory already does (one `RefDir {
    /// ensure }` just claimed; task desktop-sublist-start). A sub-list's first line then arrives
    /// through `Apply` the way a nested file's first op arrives over the LAN
    /// (`lan_apply::get_or_create_actor`), and no client has to write the file itself (design
    /// §7). Any other unknown path stays `not_found`, so a typo never creates a document.
    fn actor_or_new_list(
        &self,
        path: &FilePath,
        mutations: &[Mutation],
    ) -> Result<ActorHandle, Status> {
        if let Ok(actor) = self.actor_by_path(path) {
            return Ok(actor);
        }
        let disk = self.workspace().root().join(path.as_str());
        let dir_exists = disk.parent().is_some_and(Path::is_dir);
        let all_adds = mutations.iter().all(|m| matches!(m, Mutation::Add { .. }));
        if disk.exists() || !dir_exists || !all_adds {
            return Err(Status::not_found(format!("no document {path}")));
        }
        self.ws
            .write()
            .unwrap_or_else(PoisonError::into_inner)
            .register(path.clone())
            .map_err(|e| Status::internal(format!("register {path}: {e}")))?;
        tracing::info!(file = %path, "apply_registered_new_list");
        self.actor_by_path(path)
    }

    /// The dry run of `route_apply` (task apply-dry-run): the addressed document's own actor plans
    /// the batch and returns its diff. A cross-file `Move` is refused, since it coordinates two
    /// actors and a directory and has no single diff to show.
    pub(crate) async fn route_preview(
        &self,
        path: FilePath,
        mutations: Vec<Mutation>,
        principal: Principal,
    ) -> Result<Preview, Status> {
        if mutations.iter().any(|m| matches!(m, Mutation::Move { .. })) {
            return Err(status_of(ActorError::Mutation(MutationError::Unsupported(
                "a dry run of a cross-file Move",
            ))));
        }
        self.actor_by_path(&path)?
            .preview(mutations, principal)
            .await
            .map_err(status_of)
    }

    /// The cross-file half of `route_apply`: resolves both actors and moves the task and its
    /// `ref:` directory through `move_coordinator` (plan §3.2.8, root todo.txt task 16).
    async fn apply_move(
        &self,
        from: &FilePath,
        task: TaskRef,
        to: FilePath,
        origin: move_coordinator::Origin,
    ) -> Result<Applied, Status> {
        let source = self.actor_by_path(from)?;
        let dest = self.actor_by_path(&to)?;
        let root = self.workspace().root().to_path_buf();
        let layout = self.workspace().layout().get();
        move_coordinator::move_task_across_files(&source, &dest, task, origin, (&root, &layout))
            .await
            .map_err(status_of)
    }
}
