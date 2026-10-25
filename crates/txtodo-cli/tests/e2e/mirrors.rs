//! Task `two-kinds-of-workspace` (ADR 0043): the Remote mirrors an older build made. `doctor` says
//! what a mirror holds that the same-named clone lacks, and `workspace prune --repo-mirrors` moves
//! each aside (a dry run first) without deleting a line. A real `txtodod`, with the mirror made the
//! way an older build left it: a registered folder in the daemon's `remote/`.
#![allow(clippy::expect_used, clippy::unwrap_used)]

use crate::support;

use std::path::Path;
use support::global_daemon::GlobalDaemon;

fn stdout(out: &std::process::Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn ok(out: std::process::Output) -> String {
    let err = String::from_utf8_lossy(&out.stderr).into_owned();
    assert!(out.status.success(), "{err}");
    stdout(&out)
}

/// A folder holding `text` as its `todo.txt`, and `extra` beside it.
fn folder(root: &Path, text: &str, extra: &[(&str, &str)]) {
    std::fs::create_dir_all(root).unwrap();
    std::fs::write(root.join("todo.txt"), text).unwrap();
    for (name, body) in extra {
        std::fs::write(root.join(name), body).unwrap();
    }
}

/// The `mirror` row names the one line the clone lacks, and the fix.
fn assert_doctor_row(report: &str) {
    let row = report.lines().find(|l| l.contains("mirror")).unwrap_or("");
    let names_the_line = row.contains("1 line(s)") && row.contains("only on the mirror");
    assert!(names_the_line, "{report}");
    assert!(row.contains("workspace prune --repo-mirrors"), "{report}");
}

/// The mirror's folder went to a `.rejoin-backup-` sibling in `remote/`, with its text intact.
fn assert_backup_holds(state: &Path, text: &str) {
    let backup = std::fs::read_dir(state.join("remote"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .find(|p| p.to_string_lossy().contains(".rejoin-backup-"))
        .expect("a backup folder beside it");
    assert_eq!(
        std::fs::read_to_string(backup.join("todo.txt")).unwrap(),
        text
    );
}

#[test]
fn doctor_names_what_a_mirror_holds_and_prune_moves_it_aside_without_deleting() {
    let state = tempfile::tempdir().unwrap();
    let daemon = GlobalDaemon::spawn(state.path());
    let here = state.path();
    let work = tempfile::tempdir().unwrap();
    let clone = work.path().join("proj");
    folder(&clone, "(B) both\n", &[]);
    std::fs::create_dir_all(clone.join(".git")).unwrap();
    // The mirror shows the same name through its `txtodo.toml`, as the clone does by its folder.
    let mirror = state.path().join("remote/m1");
    let both = "(A) only on the mirror\n(B) both\n";
    folder(&mirror, both, &[("txtodo.toml", "name = \"proj\"\n")]);
    ok(daemon.txtodo(here, &["workspace", "add", &clone.display().to_string()]));
    ok(daemon.txtodo(here, &["workspace", "add", &mirror.display().to_string()]));

    assert_doctor_row(&stdout(&daemon.txtodo(here, &["doctor"])));

    let dry = ok(daemon.txtodo(here, &["workspace", "prune", "--repo-mirrors"]));
    assert!(dry.contains("run with --yes"), "{dry}");
    assert!(mirror.join("todo.txt").is_file(), "a dry run moves nothing");

    let done = ok(daemon.txtodo(here, &["workspace", "prune", "--repo-mirrors", "--yes"]));
    assert!(done.contains("moved 1 mirror(s) aside"), "{done}");
    assert!(!mirror.exists(), "the mirror folder is gone");
    assert_backup_holds(state.path(), both);

    let listed = ok(daemon.txtodo(here, &["workspace", "list"]));
    assert!(!listed.contains("remote/m1"), "{listed}");
    assert!(listed.contains("proj"), "the clone stays: {listed}");
    let again = ok(daemon.txtodo(here, &["workspace", "prune", "--repo-mirrors", "--yes"]));
    assert!(again.contains("no Remote mirrors"), "{again}");
}
