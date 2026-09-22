//! Integration test (recommended build order step 5, todo.txt item 11): `dd`/`Space`/`i`+save
//! each round-trip through a real `Daemon::apply` and repaint via a real `Watch` — a real
//! `txtodod` on a temp workspace, driven directly (not through a terminal; `app::perform` is the
//! same seam the real event loop calls).
//!
//! Unix-only (ADR 0010): a real `txtodod` means a real unix socket, same reasoning
//! `src/daemon.rs`'s own unit tests were just gated for.
//!
//! `#[ignore]`d (2026-09-19): spawns a real daemon per test; CI-only, see `tests/
//! daemon_autostart.rs`'s own doc comment for the full rationale shared across this crate's
//! real-daemon tests.
#![cfg(unix)]

mod support;

use txtodo_proto::v1 as pb;
use txtodo_tui::action::Action;
use txtodo_tui::app::perform;
use txtodo_tui::input::Input;
use txtodo_tui::state::AppState;

/// Presses one key against `state`/`input` and, if it produced an `Action`, sends it through the
/// real daemon and re-baselines — the same two steps `app::run_loop` performs per keystroke.
async fn press(
    daemon: &mut txtodo_tui::daemon::Daemon,
    input: &mut Input,
    state: &mut AppState,
    key: crossterm::event::KeyCode,
) {
    let event = crossterm::event::KeyEvent::new(key, crossterm::event::KeyModifiers::NONE);
    if let Some(action) = input.on_key(state, event) {
        perform(daemon, state, action)
            .await
            .unwrap_or_else(|e| panic!("perform: {e}"));
    }
}

#[ignore = "spawns a real txtodod; CI-only, see ci.yml's --ignored step"]
#[tokio::test]
async fn dd_deletes_a_line_through_apply_and_repaints() {
    let (_real, mut daemon) = support::RealDaemon::start("buy milk\ncall mom\n").await;
    let file = daemon
        .get_file("todo.txt")
        .await
        .unwrap_or_else(|e| panic!("get_file: {e}"));
    let mut state = AppState::from_document("todo.txt", &String::from_utf8_lossy(&file.bytes));
    let mut input = Input::default();

    assert_eq!(state.lines[0].raw, "buy milk");
    press(
        &mut daemon,
        &mut input,
        &mut state,
        crossterm::event::KeyCode::Char('d'),
    )
    .await;
    press(
        &mut daemon,
        &mut input,
        &mut state,
        crossterm::event::KeyCode::Char('d'),
    )
    .await;

    // `Delete { leave_blank: true }` (proto's own todo.sh-compatible default): the line becomes
    // blank rather than disappearing, and the daemon's repainted state reflects it immediately —
    // no separate refresh call needed, matching the design invariant that every buffer change is
    // an `Apply` the client only ever observes back through `GetFile`/`Watch`.
    assert_eq!(state.lines[0].raw, "");
    assert_eq!(state.lines[1].raw, "call mom");
}

#[ignore = "spawns a real txtodod; CI-only, see ci.yml's --ignored step"]
#[tokio::test]
async fn space_completes_a_line_through_apply() {
    let (_real, mut daemon) = support::RealDaemon::start("buy milk\n").await;
    let file = daemon
        .get_file("todo.txt")
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    let mut state = AppState::from_document("todo.txt", &String::from_utf8_lossy(&file.bytes));
    let mut input = Input::default();

    press(
        &mut daemon,
        &mut input,
        &mut state,
        crossterm::event::KeyCode::Char(' '),
    )
    .await;

    assert!(state.lines[0].completed, "line is now marked complete");
    assert!(
        state.lines[0].raw.starts_with("x "),
        "{:?}",
        state.lines[0].raw
    );
}

/// Task complete-to-bottom against a real daemon: the done line goes to the bottom of the file and
/// the cursor stays on row 0, where the next open task now is.
#[ignore = "spawns a real txtodod; CI-only, see ci.yml's --ignored step"]
#[tokio::test]
async fn space_moves_the_done_line_down_and_the_cursor_stays_on_its_row() {
    let (_real, mut daemon) = support::RealDaemon::start("buy milk\ncall mom\n").await;
    let file = daemon
        .get_file("todo.txt")
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    let mut state = AppState::from_document("todo.txt", &String::from_utf8_lossy(&file.bytes));
    let mut input = Input::default();

    press(
        &mut daemon,
        &mut input,
        &mut state,
        crossterm::event::KeyCode::Char(' '),
    )
    .await;

    assert_eq!(state.cursor, 0);
    assert_eq!(state.lines[0].raw, "call mom", "the next task slid up");
    assert!(state.lines[1].completed, "{:?}", state.lines[1].raw);
    assert!(
        state.lines[1].raw.ends_with("buy milk"),
        "{:?}",
        state.lines[1].raw
    );
}

/// root todo 9: `J` reorders the selected line to sit right after its neighbor, through a real
/// `MoveBefore` mutation and repaint.
#[ignore = "spawns a real txtodod; CI-only, see ci.yml's --ignored step"]
#[tokio::test]
async fn capital_j_reorders_the_line_after_its_neighbor() {
    let (_real, mut daemon) = support::RealDaemon::start("buy milk\ncall mom\nwalk dog\n").await;
    let file = daemon
        .get_file("todo.txt")
        .await
        .unwrap_or_else(|e| panic!("get_file: {e}"));
    let mut state = AppState::from_document("todo.txt", &String::from_utf8_lossy(&file.bytes));
    let mut input = Input::default();

    press(
        &mut daemon,
        &mut input,
        &mut state,
        crossterm::event::KeyCode::Char('J'),
    )
    .await;

    assert_eq!(state.lines[0].raw, "call mom");
    assert_eq!(state.lines[1].raw, "buy milk");
    assert_eq!(state.lines[2].raw, "walk dog");
    assert_eq!(state.cursor, 1, "cursor follows the moved line");
    assert_eq!(_real.disk(), "call mom\nbuy milk\nwalk dog\n");
}

/// The `K` half of the same feature: reorders the selected line before its neighbor.
#[ignore = "spawns a real txtodod; CI-only, see ci.yml's --ignored step"]
#[tokio::test]
async fn capital_k_reorders_the_line_before_its_neighbor() {
    let (_real, mut daemon) = support::RealDaemon::start("buy milk\ncall mom\nwalk dog\n").await;
    let file = daemon
        .get_file("todo.txt")
        .await
        .unwrap_or_else(|e| panic!("get_file: {e}"));
    let mut state = AppState::from_document("todo.txt", &String::from_utf8_lossy(&file.bytes));
    state.cursor = 1; // "call mom"
    let mut input = Input::default();

    press(
        &mut daemon,
        &mut input,
        &mut state,
        crossterm::event::KeyCode::Char('K'),
    )
    .await;

    assert_eq!(state.lines[0].raw, "call mom");
    assert_eq!(state.lines[1].raw, "buy milk");
    assert_eq!(state.lines[2].raw, "walk dog");
    assert_eq!(state.cursor, 0, "cursor follows the moved line");
    assert_eq!(_real.disk(), "call mom\nbuy milk\nwalk dog\n");
}

#[ignore = "spawns a real txtodod; CI-only, see ci.yml's --ignored step"]
#[tokio::test]
async fn i_edit_and_enter_saves_through_apply() {
    let (_real, mut daemon) = support::RealDaemon::start("buy milk\n").await;
    let file = daemon
        .get_file("todo.txt")
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    let mut state = AppState::from_document("todo.txt", &String::from_utf8_lossy(&file.bytes));
    let mut input = Input::default();

    press(
        &mut daemon,
        &mut input,
        &mut state,
        crossterm::event::KeyCode::Char('A'),
    )
    .await;
    assert!(state.editing.is_some());
    for c in " +errand".chars() {
        press(
            &mut daemon,
            &mut input,
            &mut state,
            crossterm::event::KeyCode::Char(c),
        )
        .await;
    }
    press(
        &mut daemon,
        &mut input,
        &mut state,
        crossterm::event::KeyCode::Enter,
    )
    .await;

    assert!(state.editing.is_none());
    assert_eq!(state.lines[0].raw, "buy milk +errand");
    assert_eq!(_real.disk(), "buy milk +errand\n");
}

#[ignore = "spawns a real txtodod; CI-only, see ci.yml's --ignored step"]
#[tokio::test]
async fn apply_is_visible_on_a_separate_watch_stream() {
    // "repaint via Watch": a second, independent client watching the same path sees the change
    // this session's `Apply` made — proving the daemon really did commit an op, not just answer
    // this call, and that any client (this one, or a second TUI instance) can repaint from it.
    let (_real, mut daemon) = support::RealDaemon::start("buy milk\n").await;
    let mut watcher = txtodo_tui::daemon::Daemon::connect(
        &txtodo_tui::daemon::socket_path(_real.dir.path()),
        None,
    )
    .await
    .unwrap_or_else(|e| panic!("connect: {e}"));
    let mut watch = watcher
        .watch(vec!["todo.txt".to_owned()])
        .await
        .unwrap_or_else(|e| panic!("watch: {e}"));

    let file = daemon
        .get_file("todo.txt")
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    let mut state = AppState::from_document("todo.txt", &String::from_utf8_lossy(&file.bytes));
    let mut input = Input::default();
    press(
        &mut daemon,
        &mut input,
        &mut state,
        crossterm::event::KeyCode::Char(' '),
    )
    .await;

    let change = tokio::time::timeout(std::time::Duration::from_secs(5), watch.message())
        .await
        .unwrap_or_else(|_| panic!("no Watch change arrived in time"))
        .unwrap_or_else(|e| panic!("watch stream error: {e}"))
        .unwrap_or_else(|| panic!("watch stream ended with no change"));
    assert_eq!(change.path, "todo.txt");
    assert!(
        !change.ops.is_empty(),
        "the Complete mutation produced a real op"
    );
}

/// Task workspace-layout: the TUI opens the root list the layout names, not a hard-coded todo.txt.
#[ignore = "spawns a real txtodod; CI-only, see ci.yml's --ignored step"]
#[tokio::test]
async fn the_root_list_is_the_layouts_todo_file() {
    let (_real, mut daemon) = support::RealDaemon::start_with_files(&[
        ("txtodo.toml", "todo_file = \"work.txt\"\n"),
        ("work.txt", "plan the launch\n"),
    ])
    .await;
    let root = daemon.root_list().await.unwrap();
    assert_eq!(root, "work.txt");
    let file = daemon.get_file(&root).await.unwrap();
    assert_eq!(file.bytes, b"plan the launch\n");
}

/// Root todo "tui: J/K on or beside a blank line exits the TUI with an error": `J` past a blank
/// line reorders through the daemon without addressing the blank, and a refusal the daemon does
/// make (here: an `Apply` aimed at the blank line by hand) lands on the status line instead of
/// ending the session.
#[ignore = "spawns a real txtodod; CI-only, see ci.yml's --ignored step"]
#[tokio::test]
async fn capital_j_past_a_blank_line_reorders_and_a_refusal_stays_in_the_loop() {
    let (_real, mut daemon) = support::RealDaemon::start("buy milk\n\ncall mom\n").await;
    let file = daemon
        .get_file("todo.txt")
        .await
        .unwrap_or_else(|e| panic!("get_file: {e}"));
    let mut state = AppState::from_document("todo.txt", &String::from_utf8_lossy(&file.bytes));
    let mut input = Input::default();

    let shift_j = crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Char('J'),
        crossterm::event::KeyModifiers::SHIFT,
    );
    let action = input
        .on_key(&mut state, shift_j)
        .unwrap_or_else(|| panic!("J acts"));
    let keep_going = perform(&mut daemon, &mut state, action)
        .await
        .unwrap_or_else(|e| panic!("perform: {e}"));
    assert!(
        keep_going && state.last_error.is_none(),
        "{:?}",
        state.last_error
    );
    let raws: Vec<&str> = state.lines.iter().map(|l| l.raw.as_str()).collect();
    assert_eq!(
        raws,
        ["", "call mom", "buy milk"],
        "milk moved to the end, past the blank"
    );

    // The refusal path: a mutation that names the blank line, as the old J/K did.
    let blank = pb::TaskRef {
        line_number: 1,
        task_id: String::new(),
    };
    let req = pb::ApplyRequest {
        path: "todo.txt".to_owned(),
        mutations: vec![pb::Mutation {
            kind: Some(pb::mutation::Kind::Complete(pb::Complete {
                task: Some(blank),
                today: "2026-09-23".to_owned(),
            })),
        }],
        source: "tui".to_owned(),
        ..pb::ApplyRequest::default()
    };
    let keep_going = perform(&mut daemon, &mut state, Action::Apply(req))
        .await
        .unwrap_or_else(|e| panic!("a refusal must not be an error: {e}"));
    assert!(keep_going);
    let refused = state.last_error.clone().unwrap_or_default();
    assert!(refused.contains("blank"), "{refused}");
}
