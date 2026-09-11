//! `txtodo add` / `addm` against real files: date stamp, `id:` stamp, `--no-id`, config `id_tags`.
// Integration tests are tests: clippy.toml allows unwrap/expect in #[test] fns but not in their helpers.
#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::path::Path;
use std::process::{Command, Output};

fn txtodo(dir: &Path, args: &[&str]) -> Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_txtodo"));
    cmd.current_dir(dir)
        .env_remove("TXTODO_TODO_DIR")
        .env("TXTODO_CONFIG", dir.join("none.toml"));
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
fn add_stamps_date_then_id_and_no_id_skips_the_tag() {
    let dir = tempfile::tempdir().unwrap();
    let out = txtodo(dir.path(), &["add", "(b)", "call", "mum", "+family"]);
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
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.starts_with(&format!("1 {prefix}")) && stdout.ends_with("TODO: 1 added.\n"));
    txtodo(dir.path(), &["--no-id", "a", "second"]);
    assert_eq!(
        todo_txt(dir.path()).lines().nth(1).unwrap(),
        format!("{} second", today())
    );
}

#[test]
fn addm_adds_one_task_per_line_and_config_can_disable_ids() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("cfg.toml"), "id_tags = false\n").unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_txtodo"))
        .current_dir(dir.path())
        .env("TXTODO_CONFIG", dir.path().join("cfg.toml"))
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
