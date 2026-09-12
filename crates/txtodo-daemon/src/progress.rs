//! `ListFiles` progress (plan §3.2.5): `done` = completed lines in a todo.txt plus task lines in
//! its sibling done.txt; `total` = task lines in both, blanks excluded. Split out of `server.rs`
//! (whose `list_files` delegates here) to keep that file within its line budget — the pattern
//! mirrors notes.rs's `impl TxtodoService` extension.

use crate::convert::{progress_of, sibling_done_path};
use crate::handle::ActorHandle;
use crate::server::{TxtodoService, status_of};
use tonic::Status;
use txtodo_proto::v1 as pb;

impl TxtodoService {
    /// Progress for a TODO-kind file: its own counts plus its sibling done.txt's, when tracked.
    pub(crate) async fn progress_for(&self, h: &ActorHandle) -> Result<pb::Progress, Status> {
        let todo = h.progress().await.map_err(status_of)?;
        let sibling_path = sibling_done_path(h.path());
        let sibling = self.workspace().actor(&sibling_path).cloned();
        let done = if let Some(d) = sibling {
            Some(d.progress().await.map_err(status_of)?)
        } else {
            None
        };
        Ok(progress_of(todo, done))
    }
}
