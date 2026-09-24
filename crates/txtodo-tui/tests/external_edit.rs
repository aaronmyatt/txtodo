//! Integration tests (recommended build order step 5, todo.txt item 12): an external edit (a
//! save from outside the TUI) appears on a `Watch` stream without a manual refresh, and a
//! dropped `Watch` reconnects and re-baselines. A real `txtodod` on a temp workspace.
//!
//! Unix-only (ADR 0010): a real `txtodod` means a real unix socket, same reasoning
//! `src/daemon.rs`'s own unit tests were just gated for.
//!
//! `#[ignore]`d (2026-09-19): spawns a real daemon per test; CI-only, see `tests/
//! daemon_autostart.rs`'s own doc comment for the full rationale shared across this crate's
//! real-daemon tests.
#![cfg(unix)]

mod support;

use std::time::Duration;

use txtodo_tui::app::{follow_change, reconnect_watch};
use txtodo_tui::state::AppState;

#[ignore = "spawns a real txtodod; CI-only, see ci.yml's --ignored step"]
#[tokio::test]
async fn external_edit_appears_on_watch_without_a_manual_refresh() {
    let (real, mut daemon) = support::RealDaemon::start("buy milk\n").await;
    let mut state = AppState::from_document("todo.txt", "buy milk\n");
    let mut watch = daemon
        .watch(vec!["todo.txt".to_owned()])
        .await
        .unwrap_or_else(|e| panic!("watch: {e}"));

    // An edit from "outside" the TUI entirely — no RPC call at all, just a file write, the same
    // way a text editor or another device's sync would touch the file.
    real.external_write("buy milk\ncall mom\n");

    let change = tokio::time::timeout(Duration::from_secs(10), watch.message())
        .await
        .unwrap_or_else(|_| panic!("no Watch change arrived for the external edit"))
        .unwrap_or_else(|e| panic!("watch stream error: {e}"))
        .unwrap_or_else(|| panic!("watch stream ended with no change"));
    assert_eq!(change.path, "todo.txt");

    // The client never re-reads the file itself (design invariant): following the change
    // re-baselines from `GetFile`, so the list now shows the externally added line.
    let fresh = follow_change(&mut daemon, &mut state, change)
        .await
        .unwrap_or_else(|e| panic!("follow_change: {e}"));
    assert!(fresh.is_none(), "not a layout change");
    let lines: Vec<&str> = state.lines.iter().map(|l| l.raw.as_str()).collect();
    assert_eq!(lines, ["buy milk", "call mom"]);
}

#[ignore = "spawns a real txtodod; CI-only, see ci.yml's --ignored step"]
#[tokio::test]
async fn a_dropped_watch_reconnects_and_rebaselines() {
    let (real, mut daemon) = support::RealDaemon::start("buy milk\n").await;
    let mut state = {
        let file = daemon
            .get_file("todo.txt")
            .await
            .unwrap_or_else(|e| panic!("{e}"));
        AppState::from_document("todo.txt", &String::from_utf8_lossy(&file.bytes))
    };

    // Simulate the stream dying: drop it, then make an external change while nobody is watching
    // — exactly the scenario `reconnect_watch` exists for ("a stale cursor must never paint a
    // line that no longer exists").
    {
        let dead = daemon
            .watch(vec!["todo.txt".to_owned()])
            .await
            .unwrap_or_else(|e| panic!("watch: {e}"));
        drop(dead);
    }
    real.external_write("buy milk\ncall mom\nwater the plants\n");

    // The daemon debounces external changes (plan M3); give it a moment to notice and reconcile
    // before `reconnect_watch` re-baselines — otherwise the re-baseline would just race the
    // debounce window and flake.
    loop {
        let file = daemon
            .get_file("todo.txt")
            .await
            .unwrap_or_else(|e| panic!("get_file: {e}"));
        if String::from_utf8_lossy(&file.bytes).contains("water the plants") {
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }

    let mut reconnects = 0u32;
    let watch = reconnect_watch(&mut daemon, &mut state, &mut reconnects)
        .await
        .unwrap_or_else(|e| panic!("reconnect_watch: {e}"));
    drop(watch);

    assert_eq!(reconnects, 1);
    assert_eq!(
        state.lines.len(),
        3,
        "re-baselined to the on-disk line count"
    );
    assert_eq!(state.lines[2].raw, "water the plants");
}

#[ignore = "spawns a real txtodod; CI-only, see ci.yml's --ignored step"]
#[tokio::test]
async fn reconnect_is_bounded_when_the_daemon_is_gone() {
    let (real, mut daemon) = support::RealDaemon::start("buy milk\n").await;
    let mut state = AppState::from_document("todo.txt", "buy milk");
    drop(real); // kills the daemon (RealDaemon::drop)

    let mut reconnects = 0u32;
    let mut last = Ok(());
    for _ in 0..(txtodo_tui::daemon::MAX_RECONNECT_ATTEMPTS + 1) {
        match reconnect_watch(&mut daemon, &mut state, &mut reconnects).await {
            Ok(w) => drop(w),
            Err(e) => {
                last = Err(e);
                break;
            }
        }
    }
    assert!(last.is_err(), "reconnect must give up, never loop forever");
    assert!(reconnects <= txtodo_tui::daemon::MAX_RECONNECT_ATTEMPTS + 1);
}
