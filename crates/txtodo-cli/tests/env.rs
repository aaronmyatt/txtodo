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
    assert!(line(&out, "url_schemes").starts_with("http"));
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
