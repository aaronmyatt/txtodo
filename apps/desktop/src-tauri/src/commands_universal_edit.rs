//! Completing, reopening and undoing from the Universal view (task desktop-ui-revamp). A row can
//! live in any registered workspace, so every call here names that workspace by id and goes
//! through the `*_for` client methods: the app's own open workspace (and its selector) never
//! moves. Reopen has no `Mutation` of its own; it is an `Edit` to the line the shared core's
//! completion toggle gives (`txtodo_core::chips::toggle_complete_text`, the prompt bar's `x`).

use crate::commands::ensure_connected;
use crate::dto::ApplyResultDto;
use crate::state::AppState;
use tauri::{AppHandle, State};
use txtodo_proto::v1 as pb;

fn selector(workspace_id: String) -> pb::WorkspaceSelector {
    pb::WorkspaceSelector {
        selector: Some(pb::workspace_selector::Selector::WorkspaceId(workspace_id)),
    }
}

/// Marks one row done (`done`) or not done. `today` is the client's local date (ADR 0011).
#[tracing::instrument(name = "ipc.universal_set_done", skip_all)]
#[tauri::command]
#[allow(clippy::too_many_arguments)] // one Tauri command, its args are the frontend's call shape
pub async fn universal_set_done(
    app: AppHandle,
    state: State<'_, AppState>,
    workspace_id: String,
    path: String,
    line_number: u32,
    task_id: String,
    done: bool,
    today: String,
) -> Result<ApplyResultDto, String> {
    ensure_connected(&app, &state).await?;
    let mut client = state.client_snapshot().await?;
    let sel = selector(workspace_id);
    let task = Some(pb::TaskRef {
        line_number,
        task_id,
    });
    let kind = if done {
        pb::mutation::Kind::Complete(pb::Complete { task, today })
    } else {
        let file = client
            .get_file_for(sel.clone(), &path)
            .await
            .map_err(|e| e.to_string())?;
        let text = String::from_utf8_lossy(&file.bytes);
        let new_line = reopened_line(&text, line_number, &today)
            .ok_or_else(|| format!("line {line_number} of {path} is not a done task any more"))?;
        pb::mutation::Kind::Edit(pb::Edit { task, new_line })
    };
    let req = pb::ApplyRequest {
        path,
        mutations: vec![pb::Mutation { kind: Some(kind) }],
        agent: None, // unset = the user on this device
        workspace: None,
        source: "desktop".to_owned(),
        dry_run: false,
    };
    let resp = client
        .apply_for(sel, req)
        .await
        .map_err(|e| e.apply_text())?;
    Ok(ApplyResultDto::from(resp))
}

/// Undoes the newest op on `path` in that workspace: the toast's Undo right after a toggle.
#[tracing::instrument(name = "ipc.universal_undo", skip_all)]
#[tauri::command]
pub async fn universal_undo(
    app: AppHandle,
    state: State<'_, AppState>,
    workspace_id: String,
    path: String,
) -> Result<ApplyResultDto, String> {
    ensure_connected(&app, &state).await?;
    let mut client = state.client_snapshot().await?;
    let resp = client
        .undo_for(selector(workspace_id), &path)
        .await
        .map_err(|e| e.apply_text())?;
    Ok(ApplyResultDto::from(resp))
}

/// Line `line_number` (1-based) of `text` reopened, or None when it is not a done task.
fn reopened_line(text: &str, line_number: u32, today: &str) -> Option<String> {
    let raw = text.lines().nth((line_number as usize).checked_sub(1)?)?;
    raw.starts_with("x ")
        .then(|| txtodo_core::chips::toggle_complete_text(raw, today))
}

#[cfg(test)]
mod tests {
    use super::reopened_line;

    #[test]
    fn reopening_takes_the_x_and_the_dates_back_off() {
        let text = "Open one\nx 2026-10-05 2026-10-01 Done one pri:A\n";
        assert_eq!(
            reopened_line(text, 2, "2026-10-06").as_deref(),
            Some("(A) 2026-10-01 Done one")
        );
    }

    #[test]
    fn an_open_or_missing_line_is_not_reopened() {
        assert_eq!(reopened_line("Open one\n", 1, "2026-10-06"), None);
        assert_eq!(reopened_line("Open one\n", 5, "2026-10-06"), None);
        assert_eq!(reopened_line("Open one\n", 0, "2026-10-06"), None);
    }
}
