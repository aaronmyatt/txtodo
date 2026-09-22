//! `todo_notes_get`/`todo_notes_set` gRPC bodies, split out of `grpc_write.rs` for the file budget.

use tonic::transport::Channel;
use txtodo_proto::v1 as pb;
use txtodo_proto::v1::txtodo_client::TxtodoClient;

use crate::backend::{TaskId, WorkspaceArg};
use crate::error::McpError;
use crate::grpc_convert::workspace_selector;
// One mapping for every daemon failure, metadata included (task mcp-refusal-metadata).
use crate::grpc_write::status;

/// `todo_notes_get` → daemon gRPC `GetNotes`. `line_number` is irrelevant here: `GetNotes`'s
/// `parse_required_task_id` (`notes.rs`) resolves purely by `task_id`, unlike every other RPC's
/// `TaskRef`, which needs a real line number.
pub async fn notes_get(
    mut client: TxtodoClient<Channel>,
    id: TaskId,
    workspace: WorkspaceArg,
) -> Result<String, McpError> {
    let req = pb::GetNotesRequest {
        task: Some(pb::TaskRef {
            line_number: 0,
            task_id: id,
        }),
        workspace: workspace_selector(workspace),
    };
    let rep = client.get_notes(req).await.map_err(status)?;
    Ok(String::from_utf8_lossy(&rep.into_inner().bytes).into_owned())
}

/// `todo_notes_set` → daemon gRPC `EditNotes`. Note: `edit_notes_impl` hardcodes
/// `Principal::User` regardless of caller (a pre-existing M5 gap, not introduced here) — an
/// agent's notes edits are attributed to the local user until that's wired up.
pub async fn notes_set(
    mut client: TxtodoClient<Channel>,
    id: TaskId,
    text: String,
    workspace: WorkspaceArg,
) -> Result<(), McpError> {
    let req = pb::NotesEditRequest {
        task: Some(pb::TaskRef {
            line_number: 0,
            task_id: id,
        }),
        new_text: text,
        workspace: workspace_selector(workspace),
    };
    client.edit_notes(req).await.map_err(status)?;
    Ok(())
}
