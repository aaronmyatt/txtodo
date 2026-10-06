//! Universal view (ADR 0025; task desktop-ui-revamp): every registered workspace's root-list
//! tasks (done ones too with `include_done`), from the daemon's `UniversalTasks` RPC, which
//! parses each line once for every client. This file only maps its rows to the frontend's DTO:
//! the `+`/`@` sigils back on, the description after the line's prefix, and each workspace's root
//! (from `WorkspaceList`) for switching to it. Nested `ref:` sub-lists stay out (their progress
//! rides on the parent row).

use crate::commands::ensure_connected;
use crate::daemon::DaemonClient;
use crate::dto::is_ready_or_unknown;
use crate::dto_universal::{RefProgressDto, UniversalTaskDto};
use crate::state::AppState;
use std::collections::HashMap;
use tauri::{AppHandle, State};
use txtodo_proto::v1 as pb;

/// Every task across every registered workspace's root list, workspace by workspace; with
/// `include_done`, the done ones too (the page's "show done" and its `x done` stat).
#[tracing::instrument(name = "ipc.universal_tasks", skip_all)]
#[tauri::command]
pub async fn universal_tasks(
    app: AppHandle,
    state: State<'_, AppState>,
    include_done: Option<bool>,
) -> Result<Vec<UniversalTaskDto>, String> {
    ensure_connected(&app, &state).await?;
    let mut client = state.client_snapshot().await?;
    universal_rows(&mut client, include_done.unwrap_or(false)).await
}

/// The aggregation over any connected client: the Tauri command above and the e2e bridge
/// (`src/bin/e2e_bridge/universal.rs`) both run this, so Playwright tests the real body.
pub async fn universal_rows(
    client: &mut DaemonClient,
    include_done: bool,
) -> Result<Vec<UniversalTaskDto>, String> {
    let workspaces = client.workspace_list().await.map_err(|e| e.to_string())?;
    let mut resp = client
        .universal_tasks(include_done)
        .await
        .map_err(|e| e.to_string())?;
    // The RPC covers the workspaces the daemon has open; one registered but never opened (just
    // added, say) is skipped. Open those with a plain read, then ask once more.
    let seen: std::collections::HashSet<&str> =
        resp.tasks.iter().map(|t| t.workspace_id.as_str()).collect();
    let unopened: Vec<String> = workspaces
        .iter()
        .filter(|w| {
            w.root_exists && is_ready_or_unknown(w) && !seen.contains(w.workspace_id.as_str())
        })
        .map(|w| w.workspace_id.clone())
        .collect();
    if open_all(client, &unopened).await {
        resp = client
            .universal_tasks(include_done)
            .await
            .map_err(|e| e.to_string())?;
    }
    let roots: HashMap<String, String> = workspaces
        .into_iter()
        .map(|w| (w.workspace_id, w.root))
        .collect();
    Ok(resp
        .tasks
        .into_iter()
        .map(|t| {
            let root = roots.get(&t.workspace_id).cloned().unwrap_or_default();
            row_from(t, root)
        })
        .collect())
}

/// Opens each workspace by reading its root list (the read is thrown away: the rows come from
/// the RPC); true when any opened. A workspace that fails stays out, as the RPC leaves it.
async fn open_all(client: &mut DaemonClient, ids: &[String]) -> bool {
    let mut opened = false;
    for id in ids {
        let sel = pb::WorkspaceSelector {
            selector: Some(pb::workspace_selector::Selector::WorkspaceId(id.clone())),
        };
        let root_list = client.root_list_for(sel.clone()).await;
        opened |= client.get_file_for(sel, &root_list).await.is_ok();
    }
    opened
}

/// One RPC row as the frontend reads it.
fn row_from(t: pb::UniversalTask, workspace_root: String) -> UniversalTaskDto {
    UniversalTaskDto {
        description: description_of(&t.raw),
        workspace_id: t.workspace_id,
        workspace_root,
        root_list: t.root_list,
        line_number: t.line_number,
        priority: (!t.priority.is_empty()).then_some(t.priority),
        contexts: t.contexts.into_iter().map(|c| format!("@{c}")).collect(),
        projects: t.projects.into_iter().map(|p| format!("+{p}")).collect(),
        done: t.done,
        completion_date: (!t.completion_date.is_empty()).then_some(t.completion_date),
        due: (!t.due.is_empty()).then_some(t.due),
        task_id: t.task_id,
        ref_progress: t.ref_progress.map(|p| RefProgressDto {
            done: p.done,
            total: p.total,
        }),
    }
}

/// The line's description: everything after its `x `/dates/priority prefix (txtodo-core's own
/// parse, the same the daemon used), or the raw text when it does not parse as a task.
fn description_of(raw: &str) -> String {
    let file = txtodo_core::parse_file(raw.as_bytes());
    file.lines
        .first()
        .and_then(|line| line.parse())
        .and_then(|parsed| match parsed.kind {
            txtodo_core::LineKind::Task(task) => Some(task.description.to_owned()),
            _ => None,
        })
        .unwrap_or_else(|| raw.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_row_gets_its_sigils_description_and_root_back() {
        let t = pb::UniversalTask {
            workspace_id: "W1".to_owned(),
            root_list: "todo.txt".to_owned(),
            line_number: 3,
            task_id: "T1".to_owned(),
            raw: "(A) 2026-10-01 call mum +home @phone due:2026-10-08".to_owned(),
            priority: "A".to_owned(),
            due: "2026-10-08".to_owned(),
            projects: vec!["home".to_owned()],
            contexts: vec!["phone".to_owned()],
            ref_progress: Some(pb::Progress { done: 1, total: 3 }),
            ..pb::UniversalTask::default()
        };
        let row = row_from(t, "/home/me/work".to_owned());
        assert_eq!(row.description, "call mum +home @phone due:2026-10-08");
        assert_eq!(row.workspace_root, "/home/me/work");
        assert_eq!(row.priority.as_deref(), Some("A"));
        assert_eq!(row.contexts, vec!["@phone"]);
        assert_eq!(row.projects, vec!["+home"]);
        assert_eq!(row.due.as_deref(), Some("2026-10-08"));
        assert_eq!(row.completion_date, None);
        assert_eq!(row.ref_progress, Some(RefProgressDto { done: 1, total: 3 }));
    }

    #[test]
    fn a_done_row_keeps_its_date_and_drops_the_x_from_the_description() {
        let t = pb::UniversalTask {
            raw: "x 2026-10-05 2026-10-01 pay rent".to_owned(),
            done: true,
            completion_date: "2026-10-05".to_owned(),
            ..pb::UniversalTask::default()
        };
        let row = row_from(t, String::new());
        assert!(row.done);
        assert_eq!(row.completion_date.as_deref(), Some("2026-10-05"));
        assert_eq!(row.description, "pay rent");
        assert_eq!(row.priority, None);
    }
}
