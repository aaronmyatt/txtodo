//! `:w <workspace>` against a real `txtodod` (task `tui-revamp/tui-foundation`): the client points
//! at the other workspace, opens its root list and asks the loop to re-watch; a name that picks
//! nothing says so on the status line and changes nothing. Unix-only and `#[ignore]`d like the
//! crate's other real-daemon tests.
#![cfg(unix)]

mod support;

use txtodo_tui::action::Action;
use txtodo_tui::app::perform;
use txtodo_tui::state::AppState;

#[ignore = "spawns a real txtodod; CI-only, see ci.yml's --ignored step"]
#[tokio::test]
async fn colon_w_switches_to_another_registered_workspace() {
    let (_real, mut daemon) = support::RealDaemon::start("first list line\n").await;
    let other = tempfile::Builder::new()
        .prefix("tui-switch-")
        .tempdir()
        .unwrap_or_else(|e| panic!("tempdir: {e}"));
    std::fs::write(other.path().join("todo.txt"), "second list line\n")
        .unwrap_or_else(|e| panic!("{e}"));
    let added = daemon
        .workspace_add(&other.path().display().to_string())
        .await
        .unwrap_or_else(|e| panic!("workspace_add: {e}"));
    let folder = other
        .path()
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut state = AppState::from_document("todo.txt", "first list line");

    let go = |q: &str| Action::SwitchWorkspace(q.to_owned());
    let keep = perform(&mut daemon, &mut state, go("no-such-workspace"))
        .await
        .unwrap_or_else(|e| panic!("perform: {e}"));
    assert!(keep);
    assert_eq!(state.lines[0].raw, "first list line", "nothing moved");
    assert!(
        state
            .last_error
            .as_deref()
            .is_some_and(|e| e.contains("no workspace named"))
    );

    perform(&mut daemon, &mut state, go(&folder))
        .await
        .unwrap_or_else(|e| panic!("perform: {e}"));
    assert_eq!(state.lines[0].raw, "second list line");
    assert!(state.rewatch, "the loop re-watches the new workspace");
    assert_eq!(state.workspace_label.as_deref(), Some(folder.as_str()));
    assert_eq!(state.last_error, None);
    // Later calls go to the new workspace.
    let listed = daemon
        .get_file("todo.txt")
        .await
        .unwrap_or_else(|e| panic!("get_file: {e}"));
    assert!(String::from_utf8_lossy(&listed.bytes).contains("second list line"));
    daemon
        .workspace_remove(&added.workspace_id)
        .await
        .unwrap_or_else(|e| panic!("workspace_remove: {e}"));
}
