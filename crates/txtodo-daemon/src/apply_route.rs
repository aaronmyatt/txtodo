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
use tonic::Status;
use txtodo_model::{FilePath, Principal};

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
            Ok([Mutation::Move { task, to }]) => self.apply_move(&path, task, to, principal).await,
            Ok([other]) => self
                .actor_by_path(&path)?
                .apply_from(vec![other], principal, source)
                .await
                .map_err(status_of),
            Err(mutations) if mutations.iter().any(|m| matches!(m, Mutation::Move { .. })) => {
                Err(status_of(ActorError::Mutation(MutationError::Unsupported(
                    "Move must be its own Apply batch",
                ))))
            }
            Err(mutations) => self
                .actor_by_path(&path)?
                .apply_from(mutations, principal, source)
                .await
                .map_err(status_of),
        }
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
        principal: Principal,
    ) -> Result<Applied, Status> {
        let source = self.actor_by_path(from)?;
        let dest = self.actor_by_path(&to)?;
        let root = self.workspace().root().to_path_buf();
        move_coordinator::move_task_across_files(&source, &dest, task, principal, &root)
            .await
            .map_err(status_of)
    }
}
