//! Unit tests for the workspace kind (`kind.rs`, ADR 0043).

use super::*;

fn env(vars: &[(&str, &str)]) -> RegistryEnv {
    RegistryEnv::new(
        vars.iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect(),
        PathBuf::from("/cwd"),
    )
}

#[test]
fn lists_live_beside_the_default_and_the_mirrors() {
    let e = env(&[("XDG_DATA_HOME", "/data")]);
    assert_eq!(lists_dir_for(&e), PathBuf::from("/data/txtodo/lists"));
}

#[test]
fn an_isolated_daemon_keeps_its_lists_beside_its_socket() {
    let e = env(&[
        ("XDG_DATA_HOME", "/data"),
        ("TXTODO_SOCKET", "/short/txtodod.sock"),
    ]);
    assert_eq!(lists_dir_for(&e), PathBuf::from("/short/lists"));
}

#[test]
fn the_default_and_a_folder_in_the_lists_dir_are_lists() {
    let e = env(&[("XDG_DATA_HOME", "/data")]);
    assert_eq!(
        workspace_kind(&e, Path::new("/data/txtodo/default")),
        WorkspaceKind::List
    );
    assert_eq!(
        workspace_kind(&e, Path::new("/data/txtodo/lists/work")),
        WorkspaceKind::List
    );
}

#[test]
fn any_other_folder_is_a_repo() {
    let e = env(&[("XDG_DATA_HOME", "/data")]);
    for root in [
        "/home/a/code/txtodo",
        "/data/txtodo/remote/01M4MJ4YSYDGEBJWYB3F85B3DP",
        "/data/txtodo/lists",
        "/data/txtodo/listsx/work",
    ] {
        assert_eq!(
            workspace_kind(&e, Path::new(root)),
            WorkspaceKind::Repo,
            "{root}"
        );
    }
}

#[test]
fn a_default_moved_by_the_override_is_still_a_list() {
    let e = env(&[
        ("XDG_DATA_HOME", "/data"),
        ("TXTODO_DEFAULT_WORKSPACE", "/elsewhere/mine"),
    ]);
    assert_eq!(
        workspace_kind(&e, Path::new("/elsewhere/mine")),
        WorkspaceKind::List
    );
    assert_eq!(
        workspace_kind(&e, Path::new("/data/txtodo/default")),
        WorkspaceKind::Repo,
        "the moved default's old place is no longer the default"
    );
}

/// The registry stores canonical roots; the env holds the configured path. On macOS the temp dir
/// is a symlink (`/var` to `/private/var`), and the lists dir is not made yet, so this only passes
/// when the comparison resolves the longest existing ancestor.
#[test]
fn a_canonical_root_matches_a_configured_path_that_is_not_made_yet() {
    let base = std::env::temp_dir().join(format!("txtodo-kind-{}", std::process::id()));
    std::fs::create_dir_all(&base).expect("temp dir");
    let configured = base.to_string_lossy().into_owned();
    let e = env(&[("XDG_DATA_HOME", &configured)]);
    let real = base.canonicalize().expect("canonical temp dir");
    let root = real.join("txtodo/lists/work");
    assert_eq!(workspace_kind(&e, &root), WorkspaceKind::List);
    let other = real.join("txtodo/elsewhere/work");
    assert_eq!(workspace_kind(&e, &other), WorkspaceKind::Repo);
    std::fs::remove_dir_all(&base).expect("clean up");
}

#[test]
fn kinds_print_as_clients_show_them() {
    assert_eq!(WorkspaceKind::List.as_str(), "list");
    assert_eq!(WorkspaceKind::Repo.as_str(), "repo");
}
