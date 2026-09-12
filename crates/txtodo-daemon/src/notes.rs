//! `notes.md` as a Loro text doc (plan M5, design §7): `GetNotes`/`EditNotes`. Owned end-to-end by
//! the desktop-detail-view backend task; the RPC delegates here from `server.rs` untouched.

use crate::server::TxtodoService;
use tonic::{Request, Response, Status};
use txtodo_proto::v1 as pb;

impl TxtodoService {
    /// Returns the current `notes.md` bytes for the task's `ref:` directory.
    pub(crate) async fn get_notes_impl(
        &self,
        _r: Request<pb::TaskRef>,
    ) -> Result<Response<pb::NotesDoc>, Status> {
        Err(Status::unimplemented(
            "get_notes: notes.md is not wired up yet (M5)",
        ))
    }

    /// Applies one whole-document edit to `notes.md`, lazily creating the `ref:` directory and
    /// tag on the first edit when neither exists yet (plan §3.2.4).
    pub(crate) async fn edit_notes_impl(
        &self,
        _r: Request<pb::NotesEditRequest>,
    ) -> Result<Response<pb::ApplyResponse>, Status> {
        Err(Status::unimplemented(
            "edit_notes: notes.md is not wired up yet (M5)",
        ))
    }
}
