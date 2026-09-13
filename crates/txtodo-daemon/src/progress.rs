//! `ListFiles` progress (plan §3.2.5): `done` = completed lines in a todo.txt; `total` = task
//! lines, blanks excluded. Split out of `server.rs` (whose `list_files` delegates here) to keep
//! that file within its line budget — the pattern mirrors notes.rs's `impl TxtodoService`
//! extension.

use crate::convert::progress_of;
use crate::convert::status_of;
use crate::handle::ActorHandle;
use crate::server::TxtodoService;
use tonic::Status;
use txtodo_proto::v1 as pb;

impl TxtodoService {
    /// Progress for a TODO-kind file: its own counts.
    pub(crate) async fn progress_for(&self, h: &ActorHandle) -> Result<pb::Progress, Status> {
        let todo = h.progress().await.map_err(status_of)?;
        Ok(progress_of(todo))
    }
}

/// This ref's fresh rule-5 progress for `h`, `None` for a `notes.md` path (plan M5,
/// tasks/proto-tree-progress — the same field `ListFiles` carries, used by `server.rs`'s
/// `forward_changes` to attach it to a `Watch` `Change`).
pub(crate) async fn watch_progress_of(
    svc: &TxtodoService,
    h: &ActorHandle,
) -> Option<pb::Progress> {
    if crate::convert::file_kind_of(h.path()) != pb::FileKind::Todo {
        return None;
    }
    svc.progress_for(h).await.ok()
}
