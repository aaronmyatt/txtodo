//! Task cli-no-implicit-register: a command that only reads, run in a folder no workspace holds,
//! registers nothing, makes no `.txtodo/`, changes no byte, and says how to go on. (On 2026-10-11
//! `listall` and `sub 1 listall` in an old checkout registered it and the daemon rewrote ~120 lists.)
// Integration tests are tests: clippy.toml allows unwrap/expect in #[test] fns but not in their helpers.
#![allow(clippy::expect_used, clippy::unwrap_used)]

use crate::support;

use std::path::Path;
use support::global_daemon::GlobalDaemon;

/// Every file under `dir` with its bytes, sorted.
fn snapshot(dir: &Path) -> Vec<(String, Vec<u8>)> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for entry in std::fs::read_dir(&d).unwrap() {
            let p = entry.unwrap().path();
            if p.is_dir() {
                stack.push(p);
            } else {
                let rel = p.strip_prefix(dir).unwrap().display().to_string();
                out.push((rel, std::fs::read(&p).unwrap()));
            }
        }
    }
    out.sort();
    out
}

#[test]
fn reading_an_unregistered_folder_registers_nothing() {
    let state_dir = tempfile::tempdir().unwrap();
    let daemon = GlobalDaemon::spawn(state_dir.path());
    let dir = tempfile::tempdir().unwrap();
    // An old checkout: one line still carries an `id:` tag, one has a ref.
    std::fs::write(
        dir.path().join("todo.txt"),
        "buy milk id:01ARZ3NDEKTSV4RRFFQ69G5FAV\nplan trip ref:trip\n",
    )
    .unwrap();
    std::fs::create_dir_all(dir.path().join("tasks/trip")).unwrap();
    std::fs::write(dir.path().join("tasks/trip/todo.txt"), "book flights\n").unwrap();
    let before = snapshot(dir.path());

    for args in [
        &["listall"][..],
        &["ls", "milk"],
        &["lint"],
        &["sub", "2", "listall"],
        &["--dir", "tasks/trip", "listall"],
    ] {
        let out = daemon.txtodo(dir.path(), args);
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(!out.status.success(), "{args:?} must refuse");
        assert!(err.contains("txtodo workspace add"), "{args:?}: {err}");
        assert!(err.contains("--no-daemon"), "{args:?}: {err}");
    }
    // `doctor` is no read, but asks without naming the folder and warns instead of registering it.
    let doctor = daemon.txtodo(dir.path(), &["doctor"]);
    let report = String::from_utf8_lossy(&doctor.stdout);
    let err = String::from_utf8_lossy(&doctor.stderr);
    assert!(report.contains("txtodo workspace add"), "{report}{err}");

    assert!(!dir.path().join(".txtodo").exists(), "no .txtodo/ made");
    assert_eq!(snapshot(dir.path()), before, "no byte changed");
    let list = daemon.txtodo(state_dir.path(), &["workspace", "list"]);
    let listed = String::from_utf8_lossy(&list.stdout);
    let here = std::fs::canonicalize(dir.path()).unwrap();
    assert!(!listed.contains(&here.display().to_string()), "{listed}");
}

#[test]
fn a_write_still_registers_the_folder() {
    let state_dir = tempfile::tempdir().unwrap();
    let daemon = GlobalDaemon::spawn(state_dir.path());
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("todo.txt"), "").unwrap();
    let out = daemon.txtodo(dir.path(), &["add", "call mum"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let out = daemon.txtodo(dir.path(), &["listall"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(String::from_utf8_lossy(&out.stdout).contains("call mum"));
}

/// A git checkout's root that holds a list registers on its first read (ADR 0043: a repo is never
/// offered to a peer, and the daemon keeps `.txtodo/` out of the clone's git).
#[test]
fn reading_a_git_root_with_a_list_registers_it_and_keeps_it_out_of_git() {
    let state_dir = tempfile::tempdir().unwrap();
    let daemon = GlobalDaemon::spawn(state_dir.path());
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".git/info")).unwrap();
    std::fs::write(dir.path().join("todo.txt"), "water the fern\n").unwrap();

    let out = daemon.txtodo(dir.path(), &["listall"]);
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{err}");
    assert!(String::from_utf8_lossy(&out.stdout).contains("water the fern"));

    let list = daemon.txtodo(state_dir.path(), &["workspace", "list"]);
    let listed = String::from_utf8_lossy(&list.stdout);
    let here = std::fs::canonicalize(dir.path()).unwrap();
    assert!(listed.contains(&here.display().to_string()), "{listed}");
    let exclude = std::fs::read_to_string(dir.path().join(".git/info/exclude")).unwrap();
    assert!(exclude.lines().any(|l| l == ".txtodo/"), "{exclude}");
}

/// A clone an older build already opened keeps that build's identity mode, maybe Tagged, which
/// stamps `id:` into every line on register (this repo's own backlog, 2026-10-26): a read refuses.
#[test]
fn reading_a_git_root_an_older_build_opened_registers_nothing() {
    let state_dir = tempfile::tempdir().unwrap();
    let daemon = GlobalDaemon::spawn(state_dir.path());
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".git/info")).unwrap();
    std::fs::create_dir_all(dir.path().join(".txtodo")).unwrap();
    std::fs::write(dir.path().join("todo.txt"), "water the fern\n").unwrap();

    let out = daemon.txtodo(dir.path(), &["listall"]);
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(!out.status.success(), "must refuse");
    assert!(err.contains("txtodo workspace add"), "{err}");
    let todo = std::fs::read_to_string(dir.path().join("todo.txt")).unwrap();
    assert_eq!(todo, "water the fern\n", "no byte changed");
}
