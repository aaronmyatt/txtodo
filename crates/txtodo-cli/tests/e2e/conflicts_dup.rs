//! ADR 0032 end to end through the CLI (task sync-drift/duplicate-flags): a real global-mode
//! `txtodod`, `txtodo conflicts` listing a file's duplicate lines, and `keep-newest` and `delete`
//! resolving them with the daemon's `Apply` `Delete`.
// Integration tests are tests: clippy.toml allows unwrap/expect in #[test] fns but not in their helpers.
#![allow(clippy::expect_used, clippy::unwrap_used)]
#![cfg(unix)]

use crate::support;

use support::global_daemon::GlobalDaemon;

fn stdout(out: &std::process::Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

#[test]
fn duplicate_lines_are_listed_and_keep_newest_leaves_one_copy() {
    let state_dir = tempfile::tempdir().unwrap();
    let daemon = GlobalDaemon::spawn(state_dir.path());
    let ws = tempfile::tempdir().unwrap();
    std::fs::write(ws.path().join("todo.txt"), "buy milk\nwalk\nbuy milk\n").unwrap();

    let listed = stdout(&daemon.txtodo(ws.path(), &["conflicts"]));
    assert!(
        listed.contains("duplicate line (2 copies): buy milk"),
        "{listed}"
    );
    assert!(
        listed.contains("line 1") && listed.contains("line 3"),
        "{listed}"
    );

    let refused = daemon.txtodo(ws.path(), &["conflicts", "delete", "2", "--yes"]);
    assert!(!refused.status.success(), "line 2 is no duplicate");

    let kept = daemon.txtodo(ws.path(), &["conflicts", "keep-newest", "--yes"]);
    assert!(
        kept.status.success(),
        "{}",
        String::from_utf8_lossy(&kept.stderr)
    );
    assert!(stdout(&kept).contains("1 deleted"), "{}", stdout(&kept));
    let after = stdout(&daemon.txtodo(ws.path(), &["conflicts"]));
    assert!(after.contains("no conflicts"), "{after}");
    let text = std::fs::read_to_string(ws.path().join("todo.txt")).unwrap();
    assert_eq!(text.matches("buy milk").count(), 1, "{text}");
}

#[test]
fn delete_without_a_yes_asks_and_a_closed_stdin_deletes_nothing() {
    let state_dir = tempfile::tempdir().unwrap();
    let daemon = GlobalDaemon::spawn(state_dir.path());
    let ws = tempfile::tempdir().unwrap();
    std::fs::write(ws.path().join("todo.txt"), "call mum\ncall mum\n").unwrap();

    let out = daemon.txtodo(ws.path(), &["conflicts", "delete", "1"]);
    assert!(String::from_utf8_lossy(&out.stderr).contains("[y/N]"));
    assert!(stdout(&out).contains("nothing deleted"), "{}", stdout(&out));
    let text = std::fs::read_to_string(ws.path().join("todo.txt")).unwrap();
    assert_eq!(text.matches("call mum").count(), 2, "{text}");
}
