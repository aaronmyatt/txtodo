//! `txtodo env` and the path precedence: `--dir` > `$TXTODO_TODO_DIR` > config `todo_dir` > cwd.

// Integration tests are tests: clippy.toml allows unwrap/expect in #[test] fns but not in their helpers.
#![allow(clippy::expect_used, clippy::unwrap_used)]
use std::path::Path;
use std::process::Command;

/// The built binary with a scrubbed environment; the test decides every variable it sees.
fn txtodo(cwd: &Path) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_txtodo"));
    cmd.current_dir(cwd);
    for var in [
        "TXTODO_CONFIG",
        "TXTODO_TODO_DIR",
        "TXTODO_SYNC_DIR",
        "XDG_CONFIG_HOME",
        "APPDATA",
        "HOME",
        "USERPROFILE",
    ] {
        cmd.env_remove(var);
    }
    cmd
}

fn stdout(cmd: &mut Command) -> String {
    let out = cmd.output().expect("txtodo runs");
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).expect("utf-8 stdout")
}

fn line(text: &str, key: &str) -> String {
    text.lines()
        .find_map(|l| l.strip_prefix(key).and_then(|r| r.strip_prefix('=')))
        .unwrap_or_else(|| panic!("no {key} in {text}"))
        .to_string()
}

#[test]
fn defaults_to_cwd_and_reports_missing_config() {
    let dir = tempfile::tempdir().expect("tempdir");
    // A folder with a todo.txt is a workspace, so the cwd is what a client means.
    std::fs::write(dir.path().join("todo.txt"), "").expect("seed todo.txt");
    let out = stdout(txtodo(dir.path()).args(["env"]));
    let cwd = dir.path().canonicalize().expect("canonical");
    assert_eq!(
        Path::new(&line(&out, "todo_dir"))
            .canonicalize()
            .expect("dir"),
        cwd
    );
    assert!(line(&out, "todo_file").ends_with("todo.txt"));
    assert!(line(&out, "config_file").ends_with(" (missing)"), "{out}");
    assert_eq!(
        line(&out, "id_tags"),
        "false",
        "sidecar is the default now, docs/questions.md Q2"
    );
    assert_eq!(
        line(&out, "sync_dir"),
        "(not set)",
        "sync is opt-in, unlike todo_dir there is no cwd fallback"
    );
    assert!(line(&out, "url_schemes").starts_with("http"));
}

#[test]
fn sync_dir_precedence_flag_env_config_and_validation() {
    let dir = tempfile::tempdir().expect("tempdir");
    let cfg = dir.path().join("config.toml");
    let from_config = dir.path().join("from-config-sync");
    std::fs::create_dir(&from_config).expect("mkdir");
    std::fs::write(
        &cfg,
        format!("sync_dir = {:?}\n", from_config.to_string_lossy()),
    )
    .expect("write config");
    let cfg_s = cfg.to_string_lossy().to_string();

    // config `sync_dir` alone: resolved and valid (a real, writable directory).
    let out = stdout(
        txtodo(dir.path())
            .env("TXTODO_CONFIG", &cfg_s)
            .args(["env"]),
    );
    assert!(
        line(&out, "sync_dir").ends_with("from-config-sync"),
        "{out}"
    );

    // $TXTODO_SYNC_DIR overrides config, but this directory does not exist: reported as invalid,
    // not silently ignored (validated, never asserted — CLAUDE.md §3).
    let out = stdout(
        txtodo(dir.path())
            .env("TXTODO_CONFIG", &cfg_s)
            .env("TXTODO_SYNC_DIR", "from-env-missing")
            .args(["env"]),
    );
    assert!(line(&out, "sync_dir").contains("from-env-missing"), "{out}");
    assert!(line(&out, "sync_dir").contains("(invalid:"), "{out}");

    // --sync-dir overrides both, and a real directory validates clean.
    let from_flag = dir.path().join("from-flag-sync");
    std::fs::create_dir(&from_flag).expect("mkdir");
    let out = stdout(
        txtodo(dir.path())
            .env("TXTODO_CONFIG", &cfg_s)
            .env("TXTODO_SYNC_DIR", "from-env-missing")
            .args(["--sync-dir", "from-flag-sync", "env"]),
    );
    assert_eq!(
        Path::new(&line(&out, "sync_dir"))
            .canonicalize()
            .expect("dir"),
        from_flag.canonicalize().expect("canonical")
    );

    // JSON mirrors the same value plus a null-when-fine `sync_dir_problem`.
    let out = stdout(txtodo(dir.path()).env("TXTODO_CONFIG", &cfg_s).args([
        "--sync-dir",
        "from-flag-sync",
        "--json",
        "env",
    ]));
    assert!(out.contains("\"sync_dir_problem\":null"), "{out}");
}

#[test]
fn precedence_flag_env_config_cwd() {
    let dir = tempfile::tempdir().expect("tempdir");
    let cfg = dir.path().join("config.toml");
    std::fs::write(
        &cfg,
        "todo_dir = \"from-config\"\nid_tags = false\nurl_schemes = [\"gemini\"]\n",
    )
    .expect("write config");
    let cfg_s = cfg.to_string_lossy().to_string();
    let out = stdout(
        txtodo(dir.path())
            .env("TXTODO_CONFIG", &cfg_s)
            .args(["env"]),
    );
    assert!(line(&out, "todo_dir").ends_with("from-config"), "{out}");
    assert_eq!(
        (line(&out, "id_tags"), line(&out, "url_schemes")),
        ("false".into(), "gemini".into())
    );
    assert!(!line(&out, "config_file").contains("missing"));
    let out = stdout(
        txtodo(dir.path())
            .env("TXTODO_CONFIG", &cfg_s)
            .env("TXTODO_TODO_DIR", "from-env")
            .args(["env"]),
    );
    assert!(line(&out, "todo_dir").ends_with("from-env"), "{out}");
    let out = stdout(
        txtodo(dir.path())
            .env("TXTODO_CONFIG", &cfg_s)
            .env("TXTODO_TODO_DIR", "from-env")
            .args(["--dir", "from-flag", "env"]),
    );
    assert!(line(&out, "todo_dir").ends_with("from-flag"), "{out}");
}

#[test]
fn json_env_is_one_object_and_bad_config_fails() {
    let dir = tempfile::tempdir().expect("tempdir");
    let out = stdout(txtodo(dir.path()).args(["--json", "env"]));
    assert!(
        out.starts_with("{\"todo_dir\":\"") && out.trim_end().ends_with('}'),
        "{out}"
    );
    assert!(
        out.contains("\"id_tags\":false"),
        "sidecar is the default now, docs/questions.md Q2"
    );
    let cfg = dir.path().join("bad.toml");
    std::fs::write(&cfg, "nope = 1\n").expect("write");
    let out = txtodo(dir.path())
        .env("TXTODO_CONFIG", &cfg)
        .args(["env"])
        .output()
        .expect("runs");
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("cannot read config"));
}

/// Task default-workspace: outside any workspace, with nothing naming a directory, `todo_dir` is
/// the default workspace, not the folder the command ran in.
#[test]
fn outside_a_workspace_todo_dir_is_the_default_workspace() {
    let dir = tempfile::tempdir().expect("tempdir");
    let default = dir.path().join("elsewhere-default");
    let out = stdout(
        txtodo(dir.path())
            .env("TXTODO_DEFAULT_WORKSPACE", &default)
            .args(["env"]),
    );
    assert_eq!(
        line(&out, "todo_dir"),
        default.display().to_string(),
        "{out}"
    );
}

/// Task default-workspace: `workspace default` prints the directory Finder will not show, with no
/// daemon running, and `--json` says whether it exists yet.
#[test]
fn workspace_default_prints_the_path_without_a_daemon() {
    let dir = tempfile::tempdir().expect("tempdir");
    let default = dir.path().join("the-default");
    let out = stdout(
        txtodo(dir.path())
            .env("TXTODO_DEFAULT_WORKSPACE", &default)
            .env("TXTODO_NO_AUTOSTART", "1")
            .args(["workspace", "default"]),
    );
    assert_eq!(out.trim(), default.display().to_string());
    let json = stdout(
        txtodo(dir.path())
            .env("TXTODO_DEFAULT_WORKSPACE", &default)
            .env("TXTODO_NO_AUTOSTART", "1")
            .args(["--json", "workspace", "default"]),
    );
    assert!(json.contains(r#""exists":false"#), "{json}");
}

/// `doctor` reports it too, as its own row.
#[test]
fn doctor_reports_where_the_default_workspace_lives() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join("todo.txt"), "").expect("seed todo.txt");
    let default = dir.path().join("the-default");
    let out = txtodo(dir.path())
        .env("TXTODO_DEFAULT_WORKSPACE", &default)
        .env("TXTODO_NO_AUTOSTART", "1")
        .args(["--no-daemon", "doctor"])
        .output()
        .expect("txtodo runs");
    let text = String::from_utf8_lossy(&out.stdout);
    let row = text
        .lines()
        .find(|l| l.starts_with("default"))
        .unwrap_or_else(|| panic!("{text}"));
    assert!(row.contains(&default.display().to_string()), "{row}");
    assert!(row.contains("not created yet"), "{row}");
}
