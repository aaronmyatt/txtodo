//! `txtodo add` / `addm` against real files: date stamp, `id:` stamp, `--no-id`, config `id_tags`.
// Integration tests are tests: clippy.toml allows unwrap/expect in #[test] fns but not in their helpers.
#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::path::Path;
use std::process::{Command, Output};

fn txtodo(dir: &Path, args: &[&str]) -> Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_txtodo"));
    cmd.current_dir(dir)
        // Against `dir` by name: an empty folder is no workspace, and would fall back to the default.
        .env("TXTODO_TODO_DIR", dir)
        .env("TXTODO_CONFIG", dir.join("none.toml"))
        // Isolates from any ambient *global* daemon on the machine running this suite (see
        // `tests/daemon_mode.rs::txtodo`'s own comment on this exact hazard) — this file only
        // ever wants direct-file mode, never a real, shared daemon.
        .env("XDG_DATA_HOME", dir.join(".global-home"));
    cmd.args(args)
        .output()
        .unwrap_or_else(|e| panic!("txtodo runs: {e}"))
}

fn todo_txt(dir: &Path) -> String {
    String::from_utf8(std::fs::read(dir.join("todo.txt")).unwrap_or_default()).unwrap()
}

fn today() -> String {
    jiff::Zoned::now().date().to_string()
}

#[test]
fn add_stamps_date_but_no_id_by_default() {
    // docs/questions.md Q2: sidecar is the default, so a fresh workspace with no config gets no
    // id: tag written into it at all.
    let dir = tempfile::tempdir().unwrap();
    let out = txtodo(dir.path(), &["add", "(b)", "call", "mum", "+family"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let expected = format!("(B) {} call mum +family\n", today());
    assert_eq!(todo_txt(dir.path()), expected);
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(
        stdout.starts_with(&format!("1 {}", expected.trim_end()))
            && stdout.ends_with("TODO: 1 added.\n")
    );
}

#[test]
fn add_stamps_id_when_tagged_mode_is_configured_and_no_id_still_skips_it() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("cfg.toml"), "identity_mode = \"tagged\"\n").unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_txtodo"))
        .current_dir(dir.path())
        .env("TXTODO_TODO_DIR", dir.path())
        .env("TXTODO_CONFIG", dir.path().join("cfg.toml"))
        .env("XDG_DATA_HOME", dir.path().join(".global-home"))
        .args(["add", "(b)", "call", "mum", "+family"])
        .output()
        .unwrap_or_else(|e| panic!("{e}"));
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let file = todo_txt(dir.path());
    let prefix = format!("(B) {} call mum +family id:", today());
    assert!(file.starts_with(&prefix), "{file}");
    let id = file[prefix.len()..].trim_end();
    assert!(
        id.len() == 26 && txtodo_core::Ulid::parse(id).is_some(),
        "ulid: {id}"
    );
    Command::new(env!("CARGO_BIN_EXE_txtodo"))
        .current_dir(dir.path())
        .env("TXTODO_TODO_DIR", dir.path())
        .env("TXTODO_CONFIG", dir.path().join("cfg.toml"))
        .env("XDG_DATA_HOME", dir.path().join(".global-home"))
        .args(["--no-id", "a", "second"])
        .output()
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(
        todo_txt(dir.path()).lines().nth(1).unwrap(),
        format!("{} second", today()),
        "--no-id overrides identity_mode = tagged"
    );
}

#[test]
fn addm_adds_one_task_per_line_and_config_can_disable_ids() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("cfg.toml"), "id_tags = false\n").unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_txtodo"))
        .current_dir(dir.path())
        .env("TXTODO_TODO_DIR", dir.path())
        .env("TXTODO_CONFIG", dir.path().join("cfg.toml"))
        .env("XDG_DATA_HOME", dir.path().join(".global-home"))
        .args(["addm", "first\n(A) second"])
        .output()
        .unwrap();
    assert!(out.status.success());
    assert_eq!(
        todo_txt(dir.path()),
        format!("{d} first\n(A) {d} second\n", d = today())
    );
    assert!(
        !txtodo(dir.path(), &["add", "  "]).status.success(),
        "blank input is a usage error"
    );
}
