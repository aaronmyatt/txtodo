//! `client::resolve_socket_path` (task `cli-global-socket-cwd-fallback`): the printed-socket
//! regression this task exists for — `txtodo daemon status` used to always print
//! `dir.join(SOCKET_REL)` even when the connection it actually reported on went through the
//! global socket instead, because a per-dir `.txtodo/txtodod.sock` never existed on disk at all.
//! Split out of `client.rs` for its own file-length budget (`.claude/budgets.json`'s `fileLines`),
//! the same pattern `client_bundle.rs`/`client_pairing.rs`/`client_workspace.rs` already use.

use crate::client::{SOCKET_REL, resolve_socket_path};
use crate::config::Env;
use std::collections::BTreeMap;
use std::path::PathBuf;

fn env(vars: &[(&str, &str)], cwd: &str) -> Env {
    Env::new(
        vars.iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect::<BTreeMap<_, _>>(),
        PathBuf::from(cwd),
    )
}

#[test]
fn resolves_to_the_per_dir_socket_when_a_real_file_exists_there() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let dir = tmp.path();
    std::fs::create_dir_all(dir.join(".txtodo")).expect("mkdir .txtodo");
    std::fs::write(dir.join(SOCKET_REL), b"").expect("touch fake socket file");

    let e = env(&[("XDG_DATA_HOME", "/xdg-data")], "/cwd");
    assert_eq!(resolve_socket_path(dir, &e), dir.join(SOCKET_REL));
}

/// The exact regression this task was filed for: no per-dir socket file exists (nothing was ever
/// started with `--dir` for this workspace), so the real, only-ever-dialed socket is the resolved
/// global one — `resolve_socket_path` must report that, not a `dir`-relative guess.
#[test]
fn resolves_to_the_global_socket_when_no_per_dir_socket_file_exists() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let dir = tmp.path();
    // Deliberately no `.txtodo/txtodod.sock` under `dir` — the reported bug's exact setup.

    let e = env(&[("XDG_DATA_HOME", "/xdg-data")], "/cwd");
    assert_eq!(
        resolve_socket_path(dir, &e),
        PathBuf::from("/xdg-data/txtodo/txtodod.sock")
    );
}

/// Two different workspace directories, same real global socket: the reported symptom
/// (`daemon status` from two different cwds appeared to report two different daemons) was really
/// this one path being resolved consistently — proven directly rather than via a printed string.
#[test]
fn two_different_workspace_dirs_with_no_per_dir_socket_resolve_to_the_identical_global_path() {
    let a = tempfile::tempdir().expect("tempdir a");
    let b = tempfile::tempdir().expect("tempdir b");
    let e = env(&[("XDG_DATA_HOME", "/xdg-data")], "/cwd");

    assert_eq!(
        resolve_socket_path(a.path(), &e),
        resolve_socket_path(b.path(), &e)
    );
}
