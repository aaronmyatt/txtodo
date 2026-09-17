//! logging-flow-test: a no-secrets sentinel test for this crate's newly-instrumented `cli.command`
//! span and `commands/edit.rs`'s already-done/prioritized diagnostics, the same "ZZ-SENTINEL-ZZ"
//! technique `daemon/src/lan_session_security_tests.rs:26-60` established. `txtodo-cli` is a
//! binary-only crate (no `[lib]`) that only writes JSON logs when `$TXTODO_LOG` is set
//! (`main.rs::init_telemetry`), so this drives the real `txtodo` binary as a subprocess (same
//! harness idiom as `tests/add.rs`) and reads the real rotated log file back off disk — the most
//! production-shaped of every test in this task.
// Integration tests are tests: clippy.toml allows unwrap/expect in #[test] fns but not in their helpers.
#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::path::Path;
use std::process::{Command, Output};

const SENTINEL: &str = "ZZ-SENTINEL-ZZ";

fn txtodo(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_txtodo"))
        .current_dir(dir)
        .env_remove("TXTODO_TODO_DIR")
        .env("TXTODO_CONFIG", dir.join("none.toml"))
        // Isolates from any ambient *global* daemon on the machine running this suite (see
        // `tests/daemon_mode.rs::txtodo`'s own comment on this exact hazard).
        .env("XDG_DATA_HOME", dir.join(".global-home"))
        .env("TXTODO_LOG", "debug")
        .args(args)
        .output()
        .unwrap_or_else(|e| panic!("txtodo runs: {e}"))
}

/// Every byte of every rotated JSON log file `init_telemetry` wrote under `<dir>/.txtodo/logs`.
fn all_log_bytes(dir: &Path) -> String {
    let logs_dir = dir.join(".txtodo/logs");
    let mut out = String::new();
    for entry in std::fs::read_dir(&logs_dir)
        .unwrap_or_else(|e| panic!("read_dir {}: {e}", logs_dir.display()))
    {
        let entry = entry.unwrap_or_else(|e| panic!("dir entry: {e}"));
        out.push_str(&std::fs::read_to_string(entry.path()).unwrap_or_else(|e| panic!("{e}")));
    }
    out
}

/// `commands/edit.rs::log_already_done`'s own doc: "`item` is the user's `ITEM#` argument (a line
/// number), never task text" — driven for real here against a task line whose *text* carries the
/// sentinel, so a regression that started logging the line itself (not just its number) would
/// show up as a failure.
#[test]
fn cli_command_span_and_already_done_diagnostic_never_leak_a_sentinel_task_line() {
    let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));

    let add = txtodo(dir.path(), &["add", &format!("{SENTINEL} buy milk")]);
    assert!(
        add.status.success(),
        "{}",
        String::from_utf8_lossy(&add.stderr)
    );
    let first_do = txtodo(dir.path(), &["do", "1"]);
    assert!(
        first_do.status.success(),
        "{}",
        String::from_utf8_lossy(&first_do.stderr)
    );
    // Already done: the second `do 1` hits `log_already_done` for real.
    let second_do = txtodo(dir.path(), &["do", "1"]);
    assert!(
        !second_do.status.success(),
        "an already-done item fails the run"
    );

    let logs = all_log_bytes(dir.path());
    assert!(
        !logs.is_empty(),
        "sanity: TXTODO_LOG=debug actually produced JSON log output"
    );
    assert!(
        logs.contains("cli.command"),
        "sanity: the cli.command span itself was captured: {logs}"
    );
    assert!(
        !logs.contains(SENTINEL),
        "a task line's text leaked into the CLI's own JSON logs: {logs}"
    );
}
