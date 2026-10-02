//! `archive`/`report`: completed lines move to the bottom of todo.txt, in their original relative
//! order, and never to a second done.txt file (replaces the coverage `todosh_parity.rs` dropped
//! once this app's archiving stopped matching real todo.sh's own two-file convention).
// Integration tests are tests: clippy.toml allows unwrap/expect in #[test] fns but not in their helpers.
#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::path::Path;
use std::process::Command;

fn txtodo(dir: &Path, args: &[&str]) -> bool {
    Command::new(env!("CARGO_BIN_EXE_txtodo"))
        .current_dir(dir)
        .env_remove("TXTODO_TODO_DIR")
        .env("TXTODO_CONFIG", dir.join("none.toml"))
        // Isolates from any ambient *global* daemon on the machine running this suite (see
        // `tests/daemon_mode.rs::txtodo`'s own comment on this exact hazard).
        .env("XDG_DATA_HOME", dir.join(".global-home"))
        .args(args)
        .status()
        .unwrap_or_else(|e| panic!("txtodo runs: {e}"))
        .success()
}

fn todo_text(dir: &Path) -> String {
    std::fs::read_to_string(dir.join("todo.txt")).unwrap_or_default()
}

#[test]
fn archive_moves_done_lines_to_the_bottom_in_original_order_and_drops_blanks() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("todo.txt"),
        "x 2026-01-01 a\n\nb\nx 2026-01-02 c\nd\n",
    )
    .unwrap();
    assert!(txtodo(dir.path(), &["archive"]));
    assert_eq!(
        todo_text(dir.path()),
        "b\nd\nx 2026-01-01 a\nx 2026-01-02 c\n",
        "blanks dropped, done lines moved to the bottom, relative order kept"
    );
    assert!(
        !dir.path().join("done.txt").exists(),
        "no second file is created"
    );
}

#[test]
fn archive_on_nothing_done_still_drops_blanks_and_reports_so() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("todo.txt"), "a\n\nb\n").unwrap();
    assert!(txtodo(dir.path(), &["archive"]));
    assert_eq!(todo_text(dir.path()), "a\nb\n");
}

#[test]
fn report_counts_done_lines_within_the_single_file() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("todo.txt"),
        "x 2026-01-01 a\nb\nx 2026-01-02 c\n",
    )
    .unwrap();
    assert!(txtodo(dir.path(), &["report"]));
    let report = std::fs::read_to_string(dir.path().join("report.txt")).unwrap();
    let (_, counts) = report.trim_end().split_once(' ').unwrap();
    assert_eq!(counts, "3 2", "3 total, 2 done, both counted in todo.txt");
}
