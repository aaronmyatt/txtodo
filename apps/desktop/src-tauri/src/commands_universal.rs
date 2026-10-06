//! Universal view (ADR 0025, task `desktop-universal-view`): every registered workspace's open
//! root-`todo.txt` tasks (done ones too with `include_done`, task desktop-ui-revamp), aggregated in the Tauri backend by repeated per-workspace RPC calls
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
use crate::daemon::DaemonClient;
use crate::dto::is_ready_or_unknown;
use crate::dto_universal::{RefProgressDto, UniversalTaskDto};
use crate::state::AppState;
use std::collections::HashMap;
use tauri::{AppHandle, State};
use txtodo_model::WorkspaceLayout;
use txtodo_proto::v1 as pb;

/// Every open task across every registered workspace's root `todo.txt`, unsorted; with
/// `include_done`, the done ones too (the page's "show done" and its `x done` stat).
#[tracing::instrument(name = "ipc.universal_tasks", skip_all)]
#[tauri::command]
pub async fn universal_tasks(
    app: AppHandle,
    state: State<'_, AppState>,
    include_done: Option<bool>,
) -> Result<Vec<UniversalTaskDto>, String> {
    universal_tasks_inner(app, state, include_done.unwrap_or(false)).await
}

async fn universal_tasks_inner(
    app: AppHandle,
    state: State<'_, AppState>,
    include_done: bool,
) -> Result<Vec<UniversalTaskDto>, String> {
    ensure_connected(&app, &state).await?;
    let mut client = state.client_snapshot().await?;
    universal_rows(&mut client, include_done).await
}

/// The aggregation itself, over any connected client: the Tauri command above and the e2e
/// bridge (`src/bin/e2e_bridge/universal.rs`) both run this, so Playwright tests the real body.
pub async fn universal_rows(
    client: &mut DaemonClient,
    include_done: bool,
) -> Result<Vec<UniversalTaskDto>, String> {
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
        let layout_info = client.layout_for(selector.clone()).await;
        let root_list = layout_info.todo_file.clone();
        let Ok(contents) = client.get_file_for(selector.clone(), &root_list).await else {
            continue; // this workspace's own failure never blocks the others
        };
        // `ref:` progress comes from the daemon's own counts; without them the pill just hides.
        let progress = client
            .list_files_for(selector)
            .await
            .map(|r| progress_by_path(&r.files))
            .unwrap_or_default();
        let layout = WorkspaceLayout::new(&layout_info.refs_dir, &layout_info.todo_file).ok();
        tasks.extend(
            parse_tasks(&contents.bytes, include_done)
                .into_iter()
                .map(|t| UniversalTaskDto {
                    workspace_id: ws.workspace_id.clone(),
                    workspace_root: ws.root.clone(),
                    root_list: root_list.clone(),
                    line_number: t.line_number,
                    task_id: contents
                        .task_ids
                        .get(t.line_number as usize - 1)
                        .cloned()
                        .unwrap_or_default(),
                    ref_progress: t.ref_slug.as_deref().and_then(|slug| {
                        let dir = layout.as_ref()?.ref_dir_of(slug);
                        progress.get(&format!("{dir}/todo.txt")).copied()
                    }),
                    priority: t.priority,
                    contexts: t.contexts,
                    projects: t.projects,
                    description: t.description,
                    done: t.done,
                    completion_date: t.completion_date,
                    due: t.due,
                }),
        );
    }
    Ok(tasks)
}

/// Each `todo.txt` row's `done`/`total` by path (notes rows carry no progress).
fn progress_by_path(files: &[pb::FileInfo]) -> HashMap<String, RefProgressDto> {
    files
        .iter()
        .filter_map(|f| {
            let p = f.progress.as_ref()?;
            Some((
                f.path.clone(),
                RefProgressDto {
                    done: p.done,
                    total: p.total,
                },
            ))
        })
        .collect()
}

struct ParsedTask {
    line_number: u32,
    priority: Option<String>,
    contexts: Vec<String>,
    projects: Vec<String>,
    description: String,
    done: bool,
    completion_date: Option<String>,
    due: Option<String>,
    ref_slug: Option<String>,
}

/// Every task line in `bytes` (done ones only with `include_done`), 1-based line numbers over
/// every line (blanks included, matching `TaskRefDto::line_number`'s own convention) — parsed
/// with `txtodo-core`, the same byte-preserving parser the WASM editor core uses, never a
/// hand-rolled substring scan.
fn parse_tasks(bytes: &[u8], include_done: bool) -> Vec<ParsedTask> {
    let file = txtodo_core::parse_file(bytes);
    file.lines
        .iter()
        .enumerate()
        .filter_map(|(i, line)| {
            let parsed = line.parse()?;
            let txtodo_core::LineKind::Task(task) = parsed.kind else {
                return None;
            };
            if task.completed && !include_done {
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
                projects: task.projects().map(|p| format!("+{p}")).collect(),
                description: task.description.to_owned(),
                done: task.completed,
                completion_date: task.completion_date.map(|d| d.to_string()),
                due: task.tag("due").map(str::to_owned),
                ref_slug: task.ref_slug().map(str::to_owned),
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
        let tasks = parse_tasks(bytes, false);
        assert_eq!(tasks.len(), 2);
        assert_eq!(tasks[0].line_number, 1);
        assert_eq!(tasks[0].priority.as_deref(), Some("A"));
        assert_eq!(tasks[0].contexts, vec!["@home"]);
        assert_eq!(tasks[0].projects, vec!["+work"]);
        assert_eq!(tasks[0].description, "Ship it +work @home");
        assert_eq!(tasks[1].line_number, 4);
        assert_eq!(tasks[1].priority, None);
        assert_eq!(tasks[1].contexts, vec!["@errands", "@phone"]);
    }

    #[test]
    fn include_done_adds_done_lines_with_their_dates() {
        let bytes = b"Call mum due:2026-10-08 ref:call-mum\nx 2026-09-10 2026-09-01 Done already\n";
        let tasks = parse_tasks(bytes, true);
        assert_eq!(tasks.len(), 2);
        assert!(!tasks[0].done);
        assert_eq!(tasks[0].due.as_deref(), Some("2026-10-08"));
        assert_eq!(tasks[0].ref_slug.as_deref(), Some("call-mum"));
        assert!(tasks[1].done);
        assert_eq!(tasks[1].completion_date.as_deref(), Some("2026-09-10"));
    }

    #[test]
    fn progress_is_read_from_todo_rows_only() {
        let files = [
            pb::FileInfo {
                path: "tasks/a/todo.txt".to_owned(),
                progress: Some(pb::Progress { done: 1, total: 3 }),
                ..pb::FileInfo::default()
            },
            pb::FileInfo {
                path: "tasks/a/notes.md".to_owned(),
                ..pb::FileInfo::default()
            },
        ];
        let by_path = progress_by_path(&files);
        assert_eq!(by_path.len(), 1);
        assert_eq!(
            by_path.get("tasks/a/todo.txt"),
            Some(&RefProgressDto { done: 1, total: 3 })
        );
    }
}
