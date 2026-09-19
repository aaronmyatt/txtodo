//! Universal view (ADR 0025, task `desktop-universal-view`): every registered workspace's open
//! root-`todo.txt` tasks, aggregated in the Tauri backend by repeated per-workspace RPC calls
//! plus grouping done here on the desktop client — deliberately not a new daemon RPC or a
//! `txtodo-query` service, since this view is read-only and the per-workspace count is small (one
//! extra `GetFile` per registered workspace). A workspace whose root is missing, or whose fetch
//! fails for any reason, is skipped rather than failing the whole aggregation — mirrors the
//! N-workspace isolation the daemon itself already guarantees (`test-global-daemon-acceptance`).
//!
//! Nested `ref:` sub-lists/notes are not aggregated here — a deliberate scope cut; see todo.txt's
//! `desktop-universal-view` entry for the gap. Grouping by priority and filtering by `@context`
//! both happen in the frontend, over the flat list this command returns.

use crate::commands::ensure_connected;
use crate::dto::is_ready_or_unknown;
use crate::dto_universal::UniversalTaskDto;
use crate::state::AppState;
use tauri::{AppHandle, State};
use txtodo_proto::v1 as pb;

/// Every open task across every registered workspace's root `todo.txt`, unsorted.
#[tracing::instrument(name = "ipc.universal_tasks", skip_all)]
#[tauri::command]
pub async fn universal_tasks(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Vec<UniversalTaskDto>, String> {
    universal_tasks_inner(app, state).await
}

async fn universal_tasks_inner(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Vec<UniversalTaskDto>, String> {
    ensure_connected(&app, &state).await?;
    let mut guard = state.client.lock().await;
    let client = guard.as_mut().ok_or("daemon not connected")?;
    let workspaces = client.workspace_list().await.map_err(|e| e.to_string())?;

    let mut tasks = Vec::new();
    for ws in workspaces {
        // A workspace the daemon is still opening is skipped, not promoted (see
        // `is_ready_or_unknown`); the frontend shows it as loading from `list_workspaces`.
        if !ws.root_exists || !is_ready_or_unknown(&ws) {
            continue;
        }
        let selector = pb::WorkspaceSelector {
            selector: Some(pb::workspace_selector::Selector::WorkspaceId(
                ws.workspace_id.clone(),
            )),
        };
        let Ok(contents) = client.get_file_for(selector, "todo.txt").await else {
            continue; // this workspace's own failure never blocks the others
        };
        tasks.extend(
            open_tasks(&contents.bytes)
                .into_iter()
                .map(|t| UniversalTaskDto {
                    workspace_id: ws.workspace_id.clone(),
                    workspace_root: ws.root.clone(),
                    line_number: t.line_number,
                    priority: t.priority,
                    contexts: t.contexts,
                    description: t.description,
                }),
        );
    }
    Ok(tasks)
}

struct ParsedTask {
    line_number: u32,
    priority: Option<String>,
    contexts: Vec<String>,
    description: String,
}

/// Every incomplete task line in `bytes`, 1-based line numbers over every line (blanks included,
/// matching `TaskRefDto::line_number`'s own convention) — parsed with `txtodo-core`, the same
/// byte-preserving parser the WASM editor core uses, never a hand-rolled substring scan.
fn open_tasks(bytes: &[u8]) -> Vec<ParsedTask> {
    let file = txtodo_core::parse_file(bytes);
    file.lines
        .iter()
        .enumerate()
        .filter_map(|(i, line)| {
            let parsed = line.parse()?;
            let txtodo_core::LineKind::Task(task) = parsed.kind else {
                return None;
            };
            if task.completed {
                return None;
            }
            let contexts = txtodo_core::tokenize(parsed.raw)
                .into_iter()
                .filter(|s| s.kind == txtodo_core::TokenKind::Context)
                .map(|s| parsed.raw[s.start..s.end].to_owned())
                .collect();
            Some(ParsedTask {
                line_number: (i + 1) as u32,
                priority: task.priority.map(|p| p.as_char().to_string()),
                contexts,
                description: task.description.to_owned(),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_tasks_skips_blanks_and_completed_lines() {
        let bytes = b"(A) 2026-09-11 Ship it +work @home\nx 2026-09-10 2026-09-01 Done already\n\n2026-09-11 No priority @errands @phone\n";
        let tasks = open_tasks(bytes);
        assert_eq!(tasks.len(), 2);
        assert_eq!(tasks[0].line_number, 1);
        assert_eq!(tasks[0].priority.as_deref(), Some("A"));
        assert_eq!(tasks[0].contexts, vec!["@home"]);
        assert_eq!(tasks[0].description, "Ship it +work @home");
        assert_eq!(tasks[1].line_number, 4);
        assert_eq!(tasks[1].priority, None);
        assert_eq!(tasks[1].contexts, vec!["@errands", "@phone"]);
    }
}
