//! `sub` by slug path (tasks/cli-sub-by-slug): `sub <slug> COMMAND` works the sub-list whatever
//! line its parent sits on now, a nested `a/b` path reaches the inner list, and an unknown slug is
//! an error naming the nearest one. A real `txtodod` in global mode.
// Integration tests are tests: clippy.toml allows unwrap/expect in #[test] fns but not in their helpers.
#![allow(clippy::expect_used, clippy::unwrap_used)]

use crate::support;

use std::path::Path;
use std::time::{Duration, Instant};
use support::global_daemon::GlobalDaemon;

/// `sub <slug> ls` until it shows `want` or 20 s pass: lists written straight to disk are adopted
/// by the daemon's watcher, so a listing can lag the write.
fn sub_ls_until(daemon: &GlobalDaemon, ws: &Path, slug: &str, want: &str) -> String {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let out = daemon.txtodo(ws, &["sub", slug, "ls"]);
        let text = String::from_utf8_lossy(&out.stdout).into_owned();
        if text.contains(want) || Instant::now() >= deadline {
            return text;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

#[test]
fn sub_by_slug_survives_a_line_above_completing_and_reaches_a_nested_list() {
    let state = tempfile::tempdir().unwrap();
    let daemon = GlobalDaemon::spawn(state.path());
    let ws = tempfile::tempdir().unwrap();
    std::fs::write(ws.path().join("todo.txt"), "buy milk\nplan trip ref:trip\n").unwrap();
    std::fs::create_dir_all(ws.path().join("tasks/trip/flights")).unwrap();
    std::fs::write(
        ws.path().join("tasks/trip/todo.txt"),
        "book hotel\nbook car\n",
    )
    .unwrap();
    std::fs::write(
        ws.path().join("tasks/trip/flights/todo.txt"),
        "pick seats\n",
    )
    .unwrap();
    assert!(
        daemon
            .txtodo(ws.path(), &["workspace", "add"])
            .status
            .success()
    );
    assert!(sub_ls_until(&daemon, ws.path(), "trip", "book car").contains("book car"));

    // The parent's line number moves; its slug does not.
    assert!(daemon.txtodo(ws.path(), &["do", "1"]).status.success());
    let done = daemon.txtodo(ws.path(), &["sub", "trip", "do", "2"]);
    assert!(
        done.status.success(),
        "{}",
        String::from_utf8_lossy(&done.stderr)
    );
    let trip = std::fs::read_to_string(ws.path().join("tasks/trip/todo.txt")).unwrap();
    assert!(trip.contains("x ") && trip.contains("book car"), "{trip}");

    let inner = sub_ls_until(&daemon, ws.path(), "trip/flights", "pick seats");
    assert!(inner.contains("pick seats"), "{inner}");

    let unknown = daemon.txtodo(ws.path(), &["sub", "tirp", "ls"]);
    let err = String::from_utf8_lossy(&unknown.stderr);
    assert!(!unknown.status.success());
    assert!(
        err.contains("no sub-list \"tirp\"") && err.contains("trip"),
        "{err}"
    );
}
