//! `ref:` badges against a real `txtodod` (task `tui-revamp/tui-tasks`): the Tasks rows read each
//! line's sub-list progress, or a notes mark, from the daemon's file tree. Unix-only and
//! `#[ignore]`d like this crate's other real-daemon tests.
#![cfg(unix)]

mod support;

use txtodo_tui::state::AppState;
use txtodo_tui::state_tasks::RefBadge;

#[ignore = "spawns a real txtodod; CI-only, see ci.yml's --ignored step"]
#[tokio::test]
async fn rows_get_their_sub_list_progress_and_notes_marks() {
    let (_real, mut daemon) = support::RealDaemon::start_with_files(&[
        (
            "todo.txt",
            "plan the trip ref:trip\nread ref:book\nghost ref:ghost\n",
        ),
        ("tasks/trip/todo.txt", "x book flights\npack\nhotel\n"),
        ("tasks/book/notes.md", "chapter notes\n"),
    ])
    .await;
    let mut state = AppState::from_document("todo.txt", "plan the trip ref:trip\nread ref:book");
    txtodo_tui::app_refs::refresh(&mut daemon, &mut state).await;
    assert_eq!(
        state.tasks.refs.get("trip"),
        Some(&RefBadge::Progress { done: 1, total: 3 }),
        "{:?}",
        state.tasks.refs
    );
    assert_eq!(state.tasks.refs.get("book"), Some(&RefBadge::Notes));
    assert_eq!(
        state.tasks.refs.get("ghost"),
        None,
        "a ref tag with no directory"
    );
}
