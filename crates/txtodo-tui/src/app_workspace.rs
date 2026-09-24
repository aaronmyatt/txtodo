//! Which workspace the TUI opens with no `--dir`, and the status-line label naming its kind. Split
//! out of `app.rs` for its line budget.

use txtodo_proto::v1 as pb;
use txtodo_workspace_paths::{
    RegistryEnv, WorkspaceChoice, choose_workspace, remote_workspaces_dir_for,
};

use crate::daemon::{Daemon, DaemonError};
use crate::daemon_workspace::workspace_id_selector;
use crate::state::AppState;
use crate::state_shell::MenuItem;

/// The current folder when it is a workspace, else the user's default one (task
/// default-workspace), and the status-line label that says which kind it is.
pub fn pick_workspace(cwd: std::path::PathBuf) -> (std::path::PathBuf, Option<String>) {
    let Ok(env) = RegistryEnv::from_process() else {
        return (cwd, None);
    };
    let choice = choose_workspace(&env, &cwd);
    let label = workspace_label(&env, &choice);
    (choice.path().to_path_buf(), label)
}

/// `default workspace`, or `remote workspace` inside a mirror of a paired device's workspace (task
/// remote-workspace-mirror: the daemon keeps those under its own `remote/` folder), else none.
pub fn workspace_label(env: &RegistryEnv, choice: &WorkspaceChoice) -> Option<String> {
    if choice.is_default() {
        return Some("default workspace".to_owned());
    }
    // Canonical on both sides, like the daemon's registry (macOS `/var` is `/private/var`).
    // Ref: https://doc.rust-lang.org/std/fs/fn.canonicalize.html
    let canonical = |p: &std::path::Path| p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
    let remote = canonical(&remote_workspaces_dir_for(env));
    canonical(choice.path())
        .starts_with(remote)
        .then(|| "remote workspace".to_owned())
}

/// The workspace `query` picks out of `workspaces`: its id, `default`, its folder's name (any
/// case) or its root path. An unknown or ambiguous name is an error naming the problem.
pub fn pick_by_name<'a>(
    workspaces: &'a [pb::WorkspaceInfo],
    query: &str,
) -> Result<&'a pb::WorkspaceInfo, String> {
    let folder = |w: &pb::WorkspaceInfo| {
        std::path::Path::new(&w.root)
            .file_name()
            .map(|n| n.to_string_lossy().to_lowercase())
    };
    let q = query.to_lowercase();
    let hits: Vec<&pb::WorkspaceInfo> = workspaces
        .iter()
        .filter(|w| {
            w.workspace_id == query
                || (q == "default" && w.is_default)
                || folder(w).is_some_and(|f| f == q)
                || w.root == query
        })
        .collect();
    match hits.as_slice() {
        [one] => Ok(one),
        [] => Err(format!("no workspace named {query}")),
        _ => Err(format!(
            "{query} names {} workspaces; use its id",
            hits.len()
        )),
    }
}

/// `:w <query>` (task `tui-revamp/tui-foundation`), as desktop's `switch_workspace` does it: point
/// the client at the workspace, open its root list, re-baseline, and ask the loop to re-watch.
/// A name that picks nothing, or a workspace the daemon cannot serve, goes on the status line.
pub async fn switch_workspace(
    daemon: &mut Daemon,
    state: &mut AppState,
    query: &str,
) -> Result<(), DaemonError> {
    let listed = daemon.workspace_list().await?;
    let target = match pick_by_name(&listed.workspaces, query) {
        Ok(w) => w.clone(),
        Err(message) => {
            state.last_error = Some(message);
            return Ok(());
        }
    };
    let previous = daemon.selector_for_restore();
    daemon.set_selector(Some(workspace_id_selector(&target.workspace_id)));
    let opened = match daemon.root_list().await {
        Ok(path) => daemon.get_file(&path).await.map(|file| (path, file)),
        Err(e) => Err(e),
    };
    let (path, file) = match opened {
        Ok(ok) => ok,
        Err(DaemonError::Rpc(status)) => {
            daemon.set_selector(previous);
            state.last_error = Some(status.message().to_owned());
            return Ok(());
        }
        Err(e) => return Err(e),
    };
    let fresh = AppState::from_document(path, &String::from_utf8_lossy(&file.bytes));
    state.path = fresh.path;
    state.lines = fresh.lines;
    state.cursor = 0;
    state.needs_review.clear();
    state.workspace_label = Some(workspace_title(&target));
    state.shell.root = target.root.clone();
    state.last_error = None;
    state.rewatch = true;
    Ok(())
}

/// `W`: lists the workspaces with their open counts, then opens the popup (task
/// `tui-revamp/tui-shell`). A refused list goes on the status line. The daemon counts only the
/// workspaces it has loaded (`UniversalTasks`), so the rest show no count.
pub async fn open_menu(daemon: &mut Daemon, state: &mut AppState) -> Result<(), DaemonError> {
    let listed = match daemon.workspace_list().await {
        Ok(listed) => listed,
        Err(DaemonError::Rpc(status)) => {
            state.last_error = Some(status.message().to_owned());
            return Ok(());
        }
        Err(e) => return Err(e),
    };
    let open = daemon
        .universal_tasks(false)
        .await
        .map(|r| r.tasks)
        .unwrap_or_default();
    let items = menu_items(&listed.workspaces, &open, &state.shell.root);
    crate::commands_nav::open_menu(state, items);
    Ok(())
}

/// The popup's rows: each workspace, its open tasks, whether its folder is gone, and whether it is
/// the one at `root`.
pub fn menu_items(
    workspaces: &[pb::WorkspaceInfo],
    tasks: &[pb::UniversalTask],
    root: &str,
) -> Vec<MenuItem> {
    workspaces
        .iter()
        .map(|w| MenuItem {
            id: w.workspace_id.clone(),
            name: workspace_title(w),
            open: (w.load_state == pb::WorkspaceLoadState::Ready as i32).then(|| {
                tasks
                    .iter()
                    .filter(|t| t.workspace_id == w.workspace_id && !t.done)
                    .count()
            }),
            missing: !w.root_exists,
            current: w.root == root,
        })
        .collect()
}

/// The header's name for a workspace switched to.
fn workspace_title(w: &pb::WorkspaceInfo) -> String {
    if w.is_default {
        return "default workspace".to_owned();
    }
    std::path::Path::new(&w.root)
        .file_name()
        .map_or_else(|| w.root.clone(), |n| n.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    fn env(socket: &std::path::Path) -> RegistryEnv {
        let vars = BTreeMap::from([(
            "TXTODO_SOCKET".to_owned(),
            socket.join("txtodod.sock").display().to_string(),
        )]);
        RegistryEnv::new(vars, PathBuf::from("/"))
    }

    fn info(id: &str, root: &str, is_default: bool) -> pb::WorkspaceInfo {
        pb::WorkspaceInfo {
            workspace_id: id.to_owned(),
            root: root.to_owned(),
            is_default,
            ..pb::WorkspaceInfo::default()
        }
    }

    #[test]
    fn a_workspace_is_picked_by_id_default_folder_or_root() {
        let list = [
            info("01A", "/home/u/.local/share/txtodo/default", true),
            info("01B", "/home/u/Work", false),
            info("01C", "/home/u/old/work", false),
            info("01D", "/home/u/garden", false),
        ];
        let id = |q: &str| pick_by_name(&list, q).map(|w| w.workspace_id.clone());
        assert_eq!(id("default").as_deref(), Ok("01A"));
        assert_eq!(id("01D").as_deref(), Ok("01D"));
        assert_eq!(id("GARDEN").as_deref(), Ok("01D"));
        assert_eq!(id("/home/u/Work").as_deref(), Ok("01B"));
        assert!(id("work").is_err_and(|e| e.contains("2 workspaces")));
        assert!(id("nope").is_err_and(|e| e.contains("no workspace named nope")));
    }

    #[test]
    fn menu_rows_count_open_tasks_and_mark_the_current_and_missing_ones() {
        let mut gone = info("01B", "/home/u/Work", false);
        gone.root_exists = false;
        let mut here = info("01A", "/home/u/notes", true);
        here.root_exists = true;
        here.load_state = pb::WorkspaceLoadState::Ready as i32;
        let task = |ws: &str, done| pb::UniversalTask {
            workspace_id: ws.to_owned(),
            done,
            ..pb::UniversalTask::default()
        };
        let tasks = [task("01A", false), task("01A", true), task("01A", false)];
        let rows = menu_items(&[here, gone], &tasks, "/home/u/notes");
        assert_eq!(rows[0].name, "default workspace");
        let row = |i: usize| (rows[i].open, rows[i].current, rows[i].missing);
        assert_eq!(row(0), (Some(2), true, false));
        assert_eq!(row(1), (None, false, true), "not loaded: no count");
    }

    #[test]
    fn a_folder_inside_the_mirror_folder_is_labelled_remote() {
        let state = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
        let env = env(state.path());
        let mirror = remote_workspaces_dir_for(&env).join("01BX5ZZKBKACTAV9WEVGEMMVRZ");
        std::fs::create_dir_all(&mirror).unwrap_or_else(|e| panic!("mkdir: {e}"));
        let label = workspace_label(&env, &WorkspaceChoice::Here(mirror));
        assert_eq!(label.as_deref(), Some("remote workspace"));

        let own = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
        let label = workspace_label(&env, &WorkspaceChoice::Here(own.path().to_path_buf()));
        assert_eq!(label, None, "a folder the user picked says nothing");
    }
}
