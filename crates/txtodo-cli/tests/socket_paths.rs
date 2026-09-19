//! `txtodo-daemon-launch` may not depend on `txtodo-workspace-paths` (`.claude/budgets.json`'s
//! `allowedDeps`), so it carries its own copy of the device-global default socket path — the one
//! its `ensure_daemon` compares a caller's socket against to decide whether the boot unit owns it
//! (root todo id:01M2WK5DQQ500JATEN1C410KWG). This crate depends on both, so it pins them together:
//! if the daemon's socket ever moves, this fails instead of `ensure_daemon` silently spawning a
//! second daemon next to the unit's.
// Integration tests are tests: clippy.toml allows unwrap/expect in #[test] fns but not in their helpers.
#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use txtodo_daemon_launch::default_global_socket;
use txtodo_workspace_paths::{RegistryEnv, global_socket_path};

fn env(vars: &[(&str, &str)]) -> RegistryEnv {
    let vars: BTreeMap<String, String> = vars
        .iter()
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
        .collect();
    RegistryEnv::new(vars, PathBuf::from("/cwd"))
}

#[test]
fn the_launch_crates_default_socket_is_the_one_workspace_paths_resolves() {
    let home = Path::new("/home/a");
    assert_eq!(
        default_global_socket(home, None),
        global_socket_path(&env(&[("HOME", "/home/a")]), None)
    );
    assert_eq!(
        default_global_socket(home, Some(OsStr::new("/xdg"))),
        global_socket_path(
            &env(&[("HOME", "/home/a"), ("XDG_DATA_HOME", "/xdg")]),
            None
        )
    );
}
