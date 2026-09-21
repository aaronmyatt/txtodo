//! Unit tests for the path resolvers in `lib.rs`.

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
fn explicit_override_wins() {
    let e = env(&[("TXTODO_REGISTRY_DB", "/custom/registry.db")]);
    assert_eq!(registry_db_path(&e), PathBuf::from("/custom/registry.db"));
}

#[test]
fn xdg_data_home_is_preferred_over_home_fallback() {
    let e = env(&[("XDG_DATA_HOME", "/xdg-data"), ("HOME", "/home/a")]);
    assert_eq!(
        registry_db_path(&e),
        PathBuf::from("/xdg-data/txtodo/registry.db")
    );
}

#[test]
fn falls_back_to_home_dot_local_share() {
    let e = env(&[("HOME", "/home/a")]);
    assert_eq!(
        registry_db_path(&e),
        PathBuf::from("/home/a/.local/share/txtodo/registry.db")
    );
}

#[test]
fn falls_back_to_cwd_when_nothing_resolves() {
    let e = env(&[]);
    assert_eq!(
        registry_db_path(&e),
        PathBuf::from("/cwd/txtodo/registry.db")
    );
}

#[test]
fn registry_db_path_for_prefers_override_then_legacy_dir_then_global_default() {
    let dir = PathBuf::from("/workspace");
    let e = env(&[("TXTODO_REGISTRY_DB", "/custom/registry.db")]);
    assert_eq!(
        registry_db_path_for(&e, Some(&dir)),
        PathBuf::from("/custom/registry.db"),
        "override wins even over a legacy dir"
    );
    let e = env(&[("XDG_DATA_HOME", "/xdg-data")]);
    assert_eq!(
        registry_db_path_for(&e, Some(&dir)),
        PathBuf::from("/workspace/.txtodo/registry.db"),
        "legacy dir wins over the global default when no override is set"
    );
    assert_eq!(
        registry_db_path_for(&e, None),
        PathBuf::from("/xdg-data/txtodo/registry.db"),
        "no legacy dir falls back to the true global default"
    );
}

#[test]
fn global_socket_path_prefers_override_then_legacy_dir_then_global_default() {
    let dir = PathBuf::from("/workspace");
    let e = env(&[("TXTODO_SOCKET", "/custom/txtodod.sock")]);
    assert_eq!(
        global_socket_path(&e, Some(&dir)),
        PathBuf::from("/custom/txtodod.sock")
    );
    let e = env(&[("XDG_DATA_HOME", "/xdg-data")]);
    assert_eq!(
        global_socket_path(&e, Some(&dir)),
        PathBuf::from("/workspace/.txtodo/txtodod.sock"),
        "the pre-existing per-workspace socket location, unchanged"
    );
    assert_eq!(
        global_socket_path(&e, None),
        PathBuf::from("/xdg-data/txtodo/txtodod.sock")
    );
}

#[test]
fn global_pid_and_log_paths_sit_beside_the_registry() {
    let e = env(&[("XDG_DATA_HOME", "/xdg-data")]);
    assert_eq!(
        global_pid_path(&e),
        PathBuf::from("/xdg-data/txtodo/txtodod.pid")
    );
    assert_eq!(global_log_dir(&e), PathBuf::from("/xdg-data/txtodo/logs"));
}

/// Regression test for a real bug caught running the `global_socket` integration test
/// concurrently: `global_pid_path`/`global_log_dir` used to call `data_dir(env)` directly,
/// ignoring `$TXTODO_SOCKET` — so two daemons started with isolated socket overrides (e.g. two
/// tests, or two tempdir-scoped harnesses on one real machine) still fought over the *same*
/// real `$XDG_DATA_HOME/txtodo/txtodod.pid`, and the loser refused to start with "already
/// running" even though nothing it actually owned (socket, registry) was shared.
#[test]
fn global_pid_and_log_paths_follow_the_socket_override_not_just_xdg_data_home() {
    let e = env(&[
        ("XDG_DATA_HOME", "/xdg-data"),
        ("TXTODO_SOCKET", "/tmp/isolated-a/txtodod.sock"),
    ]);
    assert_eq!(
        global_pid_path(&e),
        PathBuf::from("/tmp/isolated-a/txtodod.pid")
    );
    assert_eq!(global_log_dir(&e), PathBuf::from("/tmp/isolated-a/logs"));
}

#[test]
fn the_default_workspace_sits_beside_the_registry_on_each_os() {
    // macOS and Linux: XDG_DATA_HOME wins, else ~/.local/share.
    let e = env(&[("XDG_DATA_HOME", "/xdg-data"), ("HOME", "/home/a")]);
    assert_eq!(
        default_workspace_dir(&e),
        PathBuf::from("/xdg-data/txtodo/default")
    );
    let e = env(&[("HOME", "/Users/a")]);
    assert_eq!(
        default_workspace_dir(&e),
        PathBuf::from("/Users/a/.local/share/txtodo/default")
    );
    // Windows: %LOCALAPPDATA%, with no HOME.
    let e = env(&[("LOCALAPPDATA", "/win/AppData/Local")]);
    assert_eq!(
        default_workspace_dir(&e),
        PathBuf::from("/win/AppData/Local/txtodo/default")
    );
    // Nothing resolves: the cwd, like every other path here.
    assert_eq!(
        default_workspace_dir(&env(&[])),
        PathBuf::from("/cwd/txtodo/default")
    );
}

#[test]
fn the_default_workspace_override_wins_over_every_other_source() {
    let e = env(&[
        ("TXTODO_DEFAULT_WORKSPACE", "/custom/default"),
        ("XDG_DATA_HOME", "/xdg-data"),
        ("TXTODO_SOCKET", "/tmp/iso/txtodod.sock"),
    ]);
    assert_eq!(default_workspace_dir(&e), PathBuf::from("/custom/default"));
    assert_eq!(
        default_workspace_dir_for(&e),
        PathBuf::from("/custom/default")
    );
}

/// An isolated daemon (a test, or a harness) must not create the developer's real default.
#[test]
fn an_isolated_socket_carries_the_default_workspace_with_it() {
    let e = env(&[
        ("XDG_DATA_HOME", "/xdg-data"),
        ("TXTODO_SOCKET", "/tmp/iso/txtodod.sock"),
    ]);
    assert_eq!(
        default_workspace_dir_for(&e),
        PathBuf::from("/tmp/iso/default")
    );
    let e = env(&[("XDG_DATA_HOME", "/xdg-data")]);
    assert_eq!(
        default_workspace_dir_for(&e),
        PathBuf::from("/xdg-data/txtodo/default"),
        "no socket override: the real per-user location"
    );
}

/// A fresh directory tree under the OS temp dir (this crate has no dependencies, so no
/// `tempfile`). `mk` lists directories to create, relative to the returned root.
fn tree(name: &str, mk: &[&str]) -> PathBuf {
    let root = std::env::temp_dir().join(format!("txtodo-wp-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    for d in mk {
        std::fs::create_dir_all(root.join(d)).expect("create test dir");
    }
    root
}

#[test]
fn subdirectory_of_a_workspace_resolves_up_to_its_root() {
    let root = tree("up", &[".txtodo", "tasks/slug"]);
    assert_eq!(workspace_root_from(&root.join("tasks/slug")), root);
    assert_eq!(workspace_root_from(&root), root);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_directory_with_no_workspace_above_it_stays_as_named() {
    let root = tree("fresh", &["a/b"]);
    assert_eq!(workspace_root_from(&root.join("a/b")), root.join("a/b"));
    let _ = std::fs::remove_dir_all(&root);
}

/// A linked git worktree carries `.git` (a file there, a dir here: both count) and must not be
/// folded into the clone that happens to contain it.
#[test]
fn a_git_boundary_stops_the_walk() {
    let root = tree("git", &[".txtodo", "wt/.git", "wt/sub"]);
    assert_eq!(
        workspace_root_from(&root.join("wt/sub")),
        root.join("wt/sub")
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_folder_with_state_or_a_todo_txt_is_a_workspace_and_an_empty_one_is_not() {
    let root = tree("ws", &["stateful/.txtodo", "plain", "empty"]);
    std::fs::write(root.join("plain/todo.txt"), "x\n").unwrap();
    assert!(is_workspace_dir(&root.join("stateful")));
    assert!(is_workspace_dir(&root.join("plain")));
    std::fs::write(root.join("empty/txtodo.toml"), "todo_file = \"work.txt\"\n").unwrap();
    assert!(
        is_workspace_dir(&root.join("empty")),
        "a layout file makes it one"
    );
    std::fs::remove_file(root.join("empty/txtodo.toml")).unwrap();
    assert!(!is_workspace_dir(&root.join("empty")));
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_client_uses_the_folder_it_is_in_when_it_is_a_workspace() {
    let root = tree("here", &[".txtodo", "tasks/slug"]);
    let e = env(&[("XDG_DATA_HOME", "/xdg-data")]);
    // A sub-folder of a workspace means the workspace root.
    let choice = choose_workspace(&e, &root.join("tasks/slug"));
    assert_eq!(choice, WorkspaceChoice::Here(root.clone()));
    assert!(!choice.is_default());
    assert_eq!(choice.path(), root);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_client_outside_any_workspace_falls_back_to_the_default() {
    let root = tree("out", &["a/b"]);
    let e = env(&[("XDG_DATA_HOME", "/xdg-data")]);
    let choice = choose_workspace(&e, &root.join("a/b"));
    assert_eq!(
        choice,
        WorkspaceChoice::Default(PathBuf::from("/xdg-data/txtodo/default"))
    );
    assert!(choice.is_default());
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn the_fallback_follows_an_isolated_daemon_and_the_override() {
    let root = tree("iso", &["a"]);
    let e = env(&[("TXTODO_SOCKET", "/tmp/iso/txtodod.sock")]);
    assert_eq!(
        choose_workspace(&e, &root.join("a")).path(),
        PathBuf::from("/tmp/iso/default")
    );
    let e = env(&[("TXTODO_DEFAULT_WORKSPACE", "/mine")]);
    assert_eq!(
        choose_workspace(&e, &root.join("a")).path(),
        PathBuf::from("/mine")
    );
    let _ = std::fs::remove_dir_all(&root);
}
