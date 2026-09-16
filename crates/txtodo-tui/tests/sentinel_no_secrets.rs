//! logging-flow-test: a no-secrets sentinel test for `app.rs::perform`/`reconnect_watch`, the same
//! "ZZ-SENTINEL-ZZ" technique `daemon/src/lan_session_security_tests.rs:26-60` established. Same
//! `support::RealDaemon`/`press` harness `roundtrip.rs` already uses. `txtodo-tui` is allowed to
//! depend on `txtodo-telemetry` (`.claude/budgets.json`'s `allowedDeps`), so this reuses its shared
//! `LogSink`/`capturing_dispatch` test seam directly, the same as `mcp/tests/smoke.rs`'s own
//! sentinel test — including that test's own load-bearing note: `#[tokio::test]`'s default
//! current-thread runtime is why a thread-local `set_default` guard held across `.await` points
//! still covers every async call made through it.

mod support;

use txtodo_telemetry::testing::{LogSink, capturing_dispatch};
use txtodo_tui::app::perform;
use txtodo_tui::input::Input;
use txtodo_tui::state::AppState;

const SENTINEL: &str = "ZZ-SENTINEL-ZZ";

/// Same helper `roundtrip.rs` uses: presses one key and, if it produced an `Action`, sends it
/// through the real daemon via the real `perform` seam the event loop itself calls.
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

/// `perform`'s own doc: "never `Debug`/`Display` on `Action` itself (would print the request's
/// task line text)" — driven for real here by appending sentinel text to a line and saving, the
/// real `ApplyRequest` round trip.
#[tokio::test]
async fn perform_never_leaks_a_sentinel_task_line_into_the_tui_own_logs() {
    let sink = LogSink::new();
    let dispatch = capturing_dispatch(sink.clone(), "txtodo-tui");
    let _guard = tracing::dispatcher::set_default(&dispatch);

    let (_real, mut daemon) = support::RealDaemon::start("buy milk\n").await;
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
        crossterm::event::KeyCode::Char('A'),
    )
    .await;
    assert!(state.editing.is_some(), "sanity: append-edit mode entered");
    for c in format!(" {SENTINEL}").chars() {
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
    assert!(state.editing.is_none(), "sanity: the edit committed");
    assert!(
        state.lines[0].raw.contains(SENTINEL),
        "sanity: the sentinel really did land in the line"
    );

    drop(_guard);
    let text = sink.captured_text();
    assert!(
        !text.is_empty(),
        "sanity: perform actually logged something (the tui.perform span)"
    );
    assert!(
        !text.contains(SENTINEL),
        "a task line's text leaked into the TUI's own logs: {text}"
    );
}
