//! `notes.md` commands (plan M5, design §7): `GetNotes`/`EditNotes`. Split out of `commands.rs`
//! the way `crates/txtodo-daemon/src/server.rs` delegates to `notes.rs`; same
//! `ensure_connected` + lock + map-err-to-String pattern as every command there.

use crate::commands::ensure_connected;
use crate::dto::{ApplyResultDto, NotesDocDto, TaskRefDto};
use crate::state::AppState;
use tauri::{AppHandle, State};
use txtodo_proto::v1 as pb;

/// `notes.md` for one task's `ref:` directory; the daemon resolves the task, this bridge never
/// touches the filesystem itself.
#[tracing::instrument(name = "ipc.get_notes", skip_all)]
#[tauri::command]
pub async fn get_notes(
    app: AppHandle,
    state: State<'_, AppState>,
    task: TaskRefDto,
) -> Result<NotesDocDto, String> {
    get_notes_inner(app, state, task).await
}

async fn get_notes_inner(
    app: AppHandle,
    state: State<'_, AppState>,
    task: TaskRefDto,
) -> Result<NotesDocDto, String> {
    ensure_connected(&app, &state).await?;
    let mut client = state.client_snapshot().await?;
    let resp = client
        .get_notes(task.into())
        .await
        .map_err(|e| e.to_string())?;
    Ok(NotesDocDto::from(resp))
}

/// Whole-document replacement of one task's `notes.md`; the daemon derives the Loro text ops and
/// lazily creates the `ref:` directory on the first edit (plan §3.2.4).
#[tracing::instrument(name = "ipc.edit_notes", skip_all)]
#[tauri::command]
pub async fn edit_notes(
    app: AppHandle,
    state: State<'_, AppState>,
    task: TaskRefDto,
    new_text: String,
) -> Result<ApplyResultDto, String> {
    edit_notes_inner(app, state, task, new_text).await
}

async fn edit_notes_inner(
    app: AppHandle,
    state: State<'_, AppState>,
    task: TaskRefDto,
    new_text: String,
) -> Result<ApplyResultDto, String> {
    ensure_connected(&app, &state).await?;
    let mut client = state.client_snapshot().await?;
    let req = pb::NotesEditRequest {
        task: Some(task.into()),
        new_text,
        workspace: None,
    };
    let resp = client.edit_notes(req).await.map_err(|e| e.to_string())?;
    Ok(ApplyResultDto::from(resp))
}
