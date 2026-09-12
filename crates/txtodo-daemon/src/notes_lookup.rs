//! Resolving a bare task id to the document that holds it (plan M5): `GetNotes`/`EditNotes`'s
//! wire `TaskRef` carries no path (unlike every other RPC's `TaskRef`, always paired with a
//! sibling `path` field), so the daemon asks each actor in turn "do you hold this task" rather
//! than requiring the client to resolve a document first — `notes.rs`'s module doc explains why.
//! Moved out of `handle.rs` purely to keep that file within its line budget; `ask` is
//! `pub(crate)` for exactly this sibling-module use, same as `refdir.rs`'s extension.

use crate::actor::FileActor;
use crate::handle::{ActorError, ActorHandle, ActorMsg};
use crate::refdir::task_view;
use txtodo_model::TaskId;

/// Where a task sits in its document, when it holds one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskLineInfo {
    /// 1-based line, for a fresh `TaskRef` back into this same document.
    pub line_number: usize,
    /// The task's current `ref:` slug, if it has one yet.
    pub slug: Option<String>,
}

impl ActorHandle {
    /// Whether this document holds `task_id`, and if so its line and `ref:` slug.
    pub async fn task_line(&self, task_id: TaskId) -> Result<Option<TaskLineInfo>, ActorError> {
        self.ask(|reply| ActorMsg::TaskLine { task_id, reply })
            .await
    }
}

/// Handles `TaskLine` and hands everything else back unchanged — split out of `actor.rs::handle`
/// so that function stays under its file's line budget, same pattern as `refdir_ops.rs`'s
/// `handle_refdir`.
pub(crate) fn handle_task_line(actor: &FileActor, msg: ActorMsg) -> Option<ActorMsg> {
    let ActorMsg::TaskLine { task_id, reply } = msg else {
        return Some(msg);
    };
    let _ = reply.send(task_line_info(actor, task_id));
    None
}

fn task_line_info(actor: &FileActor, task_id: TaskId) -> Option<TaskLineInfo> {
    let i = actor.state.index_of(task_id)?;
    let line = actor.state.line_of(task_id)?;
    let slug = task_view(&line).and_then(|t| t.ref_slug().map(str::to_owned));
    Some(TaskLineInfo {
        line_number: i + 1,
        slug,
    })
}
