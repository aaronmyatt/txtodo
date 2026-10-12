//! Read-only commands never register a folder (tasks/cli-no-implicit-register): the daemon opens
//! and registers any root a request names by path, so a `listall` in an old checkout made it a
//! workspace and the daemon stamped `id:` into every list there. In an unregistered folder the
//! reads fail with the fix, register nothing, make no `.txtodo/` and change no bytes. A real
//! `txtodod` in global mode.
// Integration tests are tests: clippy.toml allows unwrap/expect in #[test] fns but not in their helpers.
#![allow(clippy::expect_used, clippy::unwrap_used)]

use crate::support;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use support::global_daemon::GlobalDaemon;

/// Every file under `root` and its bytes.
fn snapshot(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    let mut out = BTreeMap::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                stack.push(path);
            } else {
                out.insert(path.clone(), std::fs::read(&path).unwrap());
            }
        }
    }
    out
}

#[test]
fn reads_in_an_unregistered_folder_register_nothing_and_change_no_bytes() {
    let state = tempfile::tempdir().unwrap();
    let daemon = GlobalDaemon::spawn(state.path());
    let ws = tempfile::tempdir().unwrap();
    std::fs::write(
        ws.path().join("todo.txt"),
        "(A) buy milk\nplan trip ref:trip\n",
    )
    .unwrap();
    std::fs::create_dir_all(ws.path().join("tasks/trip")).unwrap();
    std::fs::write(ws.path().join("tasks/trip/todo.txt"), "book flights\n").unwrap();
    let before = snapshot(ws.path());

    for args in [&["list"][..], &["listall"], &["sub", "2", "listall"]] {
        let out = daemon.txtodo(ws.path(), args);
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(!out.status.success(), "{args:?} must fail: {err}");
        assert!(
            err.contains("not a registered workspace"),
            "{args:?}: {err}"
        );
        assert!(
            err.contains("txtodo workspace add"),
            "{args:?} names the fix: {err}"
        );
    }
    // `doctor` reports, but must not register the folder either.
    let _ = daemon.txtodo(ws.path(), &["doctor"]);

    assert!(!ws.path().join(".txtodo").exists(), "no store was made");
    assert_eq!(snapshot(ws.path()), before, "no byte changed");
    let listed = daemon.txtodo(state.path(), &["workspace", "list"]);
    let root = std::fs::canonicalize(ws.path()).unwrap();
    assert!(
        !String::from_utf8_lossy(&listed.stdout).contains(&root.display().to_string()),
        "the folder is not in the registry"
    );
    // The direct-file escape hatch the error names still reads it.
    let direct = daemon.txtodo(ws.path(), &["--no-daemon", "listall"]);
    assert!(String::from_utf8_lossy(&direct.stdout).contains("buy milk"));
}
