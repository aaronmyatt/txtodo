//! `TxtodoService`'s actor lookups (`actor`, `actor_by_path`, `all_actors`) — an `impl
//! TxtodoService` extension split out of `server.rs` purely to keep that file within its line
//! budget, the same pattern as `progress.rs`/`watch_forward.rs`.

use crate::convert::parse_path;
use crate::handle::ActorHandle;
use crate::server::TxtodoService;
use tonic::Status;
use txtodo_model::FilePath;

impl TxtodoService {
    pub(crate) fn actor(&self, path: &str) -> Result<ActorHandle, Status> {
        let path = parse_path(path)?;
        self.actor_by_path(&path)
    }

    pub(crate) fn actor_by_path(&self, path: &FilePath) -> Result<ActorHandle, Status> {
        self.workspace()
            .actor(path)
            .cloned()
            .ok_or_else(|| Status::not_found(format!("no document {path}")))
    }

    pub(crate) fn all_actors(&self) -> Vec<ActorHandle> {
        let ws = self.workspace();
        ws.paths().filter_map(|p| ws.actor(p).cloned()).collect()
    }
}

/// This bare, single-workspace `TxtodoService` (whitebox tests only — production always goes
/// through `GlobalService`) has no registry to answer the workspace-management RPCs with.
pub(crate) fn no_registry() -> Status {
    Status::unimplemented("workspace management needs the global daemon's registry")
}
