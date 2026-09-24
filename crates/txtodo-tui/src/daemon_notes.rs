//! A line's `ref:` directory and its `notes.md` (task `tui-revamp/tui-foundation`), for the
//! detail panel. Split out of `daemon.rs` by area.

use txtodo_proto::v1 as pb;

use crate::daemon::{Daemon, DaemonError};

impl Daemon {
    /// The `notes.md` of `task`'s `ref:` directory (empty text when it has none yet).
    pub async fn get_notes(&mut self, task: pb::TaskRef) -> Result<pb::NotesDoc, DaemonError> {
        let req = pb::GetNotesRequest {
            task: Some(task),
            workspace: self.selector.clone(),
        };
        Ok(self.inner.get_notes(req).await?.into_inner())
    }

    /// Replaces the whole `notes.md` text; the daemon derives the text ops and creates the ref
    /// directory lazily.
    pub async fn edit_notes(
        &mut self,
        task: pb::TaskRef,
        new_text: &str,
    ) -> Result<pb::ApplyResponse, DaemonError> {
        let req = pb::NotesEditRequest {
            task: Some(task),
            new_text: new_text.to_owned(),
            workspace: self.selector.clone(),
        };
        Ok(self.inner.edit_notes(req).await?.into_inner())
    }

    /// Where `task`'s ref directory is (or would be); `ensure` creates it like a first write.
    pub async fn ref_dir(
        &mut self,
        path: &str,
        task: pb::TaskRef,
        ensure: bool,
    ) -> Result<pb::RefDirInfo, DaemonError> {
        let req = pb::RefDirRequest {
            path: path.to_owned(),
            task: Some(task),
            ensure,
            workspace: self.selector.clone(),
        };
        Ok(self.inner.ref_dir(req).await?.into_inner())
    }
}
