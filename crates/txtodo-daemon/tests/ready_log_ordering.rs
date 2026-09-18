//! `ref:daemon-ready-log-ordering`: the daemon must never claim `daemon_ready` before its socket
//! actually accepts connections. Before this task, `main.rs` logged that event straight after
//! computing the socket path, ahead of the real `UnixListener::bind` in `serve.rs`. The fix split
//! the boot-time log into `daemon_starting` (main.rs, pre-bind) and moved the real `daemon_ready`
//! event into `serve.rs::log_socket_bound`, emitted only once `bind` succeeds.
//!
//! This test doesn't need to race the bind by hand: `support::Daemon::start` already blocks on a
//! real, retried socket connect (`support::connect`) before returning, so a passing spawn is proof
//! the socket was live. What this test adds is the regression guard a future refactor could
//! silently break: `daemon_starting` must still precede `daemon_ready` in the daemon's own log.
#![allow(clippy::expect_used, clippy::unwrap_used)]
#![cfg(unix)]

mod support;

use support::Daemon;

/// Same JSON-log parsing shape as `logging_flow_sequence.rs::event_names_in_order` — the
/// `fields.message` field is what every bare `tracing::info!("some_name")` call lands under in
/// this workspace's JSON layer.
fn event_names_in_order(log: &str) -> Vec<String> {
    let mut names = Vec::new();
    for line in log.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with("---") {
            continue;
        }
        let Ok(value) = serde_json::from_str::<serde_json::Value>(trimmed) else {
            continue;
        };
        let Some(name) = value
            .get("fields")
            .and_then(|f| f.get("message"))
            .and_then(|m| m.as_str())
        else {
            continue;
        };
        names.push(name.to_owned());
    }
    names
}

#[tokio::test]
async fn daemon_starting_precedes_daemon_ready_in_the_log() {
    let daemon = Daemon::start("").await;
    let events = event_names_in_order(&daemon.log_tail());

    let starting = events.iter().position(|e| e == "daemon_starting");
    let ready = events.iter().position(|e| e == "daemon_ready");

    assert!(
        starting.is_some(),
        "expected a daemon_starting event; saw: {events:?}"
    );
    assert!(
        ready.is_some(),
        "expected a daemon_ready event; saw: {events:?}"
    );
    assert!(
        starting < ready,
        "daemon_starting must precede daemon_ready; saw: {events:?}"
    );
}
