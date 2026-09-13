//! Differential parity (plan M2 acceptance): every scenario runs through the vendored todo.sh
//! (v2.14.0, `-f -t -p`, default config) and through `txtodo --no-id`; afterwards todo.txt, done.txt
//! and other.txt must be byte-identical and both runs must agree on success. Auto-archive-after-
//! `do` is suppressed for both tools (`run_todo_sh`'s own doc comment): this app's `archive` moves
//! a completed task within todo.txt, never to a second done.txt, so it is no longer comparable
//! byte-for-byte with real todo.sh's archiving — done.txt only ever holds each scenario's seed
//! here, unchanged by either tool. Needs bash, sed and date, so it is compiled out on Windows.
#![cfg(not(windows))]
// Integration tests are tests: clippy.toml allows unwrap/expect in #[test] fns but not in their helpers.
#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::path::Path;
use std::process::Command;

/// Seed files, then the commands to run against them in order.
struct Scenario {
    name: &'static str,
    todo: &'static str,
    done: &'static str,
    other: &'static str,
    steps: &'static [&'static [&'static str]],
}

const fn s(
    name: &'static str,
    todo: &'static str,
    steps: &'static [&'static [&'static str]],
) -> Scenario {
    Scenario {
        name,
        todo,
        done: "",
        other: "",
        steps,
    }
}

const SCENARIOS: &[Scenario] = &[
    s("add-plain", "", &[&["add", "call mum"]]),
    s(
        "add-priority-lowercase",
        "",
        &[&["add", "(b) call mum +family @phone"]],
    ),
    s("add-many-args", "", &[&["add", "call", "mum", "tonight"]]),
    s(
        "add-no-trailing-newline",
        "first\nsecond",
        &[&["add", "third"]],
    ),
    s(
        "add-two-then-list",
        "a\n",
        &[&["add", "b"], &["add", "(A) c"], &["ls"]],
    ),
    s("addm", "", &[&["addm", "one\ntwo"]]),
    s("addm-priorities", "z\n", &[&["addm", "(a) one\n(B) two"]]),
    s("do-single", "a\nb\n", &[&["do", "1"]]),
    s("do-multiple-comma", "a\nb\nc\n", &[&["do", "1,3"]]),
    s("do-multiple-args", "a\nb\nc\n", &[&["do", "2", "3"]]),
    s("do-already-done", "x 2026-01-01 a\nb\n", &[&["do", "1"]]),
    s("do-leaves-a-blank-line-alone", "a\n\nb\n", &[&["do", "3"]]),
    s(
        "do-then-add",
        "a\n",
        &[&["add", "b"], &["do", "1"], &["add", "c"]],
    ),
    s("archive-nothing-done", "a\n\nb\n", &[&["archive"]]),
    s("del-line", "a\nb\nc\n", &[&["del", "2"]]),
    s(
        "del-term-middle",
        "(A) foo bar baz\n",
        &[&["del", "1", "bar"]],
    ),
    s(
        "del-term-start-then-end",
        "(A) foo bar baz\n",
        &[&["rm", "1", "foo"], &["del", "1", "baz"]],
    ),
    s(
        "del-term-missing",
        "(A) foo bar baz\n",
        &[&["del", "1", "zzz"]],
    ),
    s("pri-new", "a\n", &[&["pri", "1", "b"]]),
    s("pri-change", "(A) a\n", &[&["p", "1", "c"]]),
    s("pri-same", "(A) a\n", &[&["pri", "1", "a"]]),
    s("pri-pairs", "a\nb\n", &[&["pri", "1", "a", "2", "b"]]),
    s("depri", "(A) a\nb\n", &[&["depri", "1"], &["dp", "2"]]),
    s("append-space", "a\n", &[&["app", "1", "more"]]),
    s("append-delimiter", "a\n", &[&["append", "1", ", more"]]),
    s(
        "prepend-keeps-prefix",
        "(A) 2026-09-11 rest\n",
        &[&["prep", "1", "new"]],
    ),
    s("prepend-plain", "rest\n", &[&["prepend", "1", "new"]]),
    s(
        "replace-plain",
        "(A) 2026-09-11 old\n",
        &[&["replace", "1", "new"]],
    ),
    s(
        "replace-with-date",
        "(A) 2026-09-11 old\n",
        &[&["replace", "1", "2020-01-01 new"]],
    ),
    s(
        "replace-with-priority",
        "(A) 2026-09-11 old\n",
        &[&["replace", "1", "(C) new"]],
    ),
    s("move", "a\nb\n", &[&["mv", "1", "other.txt"]]),
    Scenario {
        name: "move-to-unterminated-dest",
        todo: "a\nb\n",
        done: "",
        other: "z",
        steps: &[&["move", "2", "other.txt"]],
    },
    s("deduplicate", "a\nb\na\n\nb\n", &[&["deduplicate"]]),
    s("deduplicate-none", "a\nb\n", &[&["deduplicate"]]),
    Scenario {
        name: "listings-change-nothing",
        todo: "(A) a +work @desk\n\nb +home\nx 2026-01-01 c\n",
        done: "x 2025-12-31 z\n",
        other: "",
        steps: &[
            &["ls"],
            &["ls", "+work"],
            &["lsa"],
            &["lsp"],
            &["lsp", "A"],
            &["lsprj"],
            &["lsc"],
            &["lf"],
            &["lf", "done.txt"],
        ],
    },
];

const TODO_SH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/vendor/todo.sh");

fn seed(dir: &Path, sc: &Scenario) {
    std::fs::write(dir.join("todo.txt"), sc.todo).unwrap();
    std::fs::write(dir.join("done.txt"), sc.done).unwrap();
    std::fs::write(dir.join("other.txt"), sc.other).unwrap();
    let cfg = format!(
        "export TODO_DIR=\"{d}\"\nexport TODO_FILE=\"$TODO_DIR/todo.txt\"\nexport DONE_FILE=\"$TODO_DIR/done.txt\"\nexport REPORT_FILE=\"$TODO_DIR/report.txt\"\n",
        d = dir.display()
    );
    std::fs::write(dir.join("todo.cfg"), cfg).unwrap();
}

/// Both tools see the same minimal environment: PATH for bash/sed/date, HOME inside the temp dir.
fn command(program: &str, dir: &Path) -> Command {
    let mut cmd = Command::new(program);
    cmd.env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("HOME", dir)
        .current_dir(dir);
    cmd
}

// `-a`/`-A` suppress each tool's own auto-archive-after-`do` (opposite spellings: real todo.sh's
// `-a` disables it, this app's `-A`/`--no-archive` does). Archiving itself now diverges by design
// (this app moves a completed task within todo.txt; todo.sh moves it to a second done.txt), so
// `do` scenarios below stay comparable only with archiving out of the picture; `archive`/`report`
// invoked directly are covered by this crate's own tests instead of this differential harness.
fn run_todo_sh(dir: &Path, step: &[&str]) -> bool {
    let cfg = dir.join("todo.cfg");
    let out = command(TODO_SH, dir)
        .args(["-d", cfg.to_str().unwrap(), "-f", "-t", "-p", "-a"])
        .args(step)
        .output()
        .unwrap();
    out.status.success()
}

fn run_txtodo(dir: &Path, step: &[&str]) -> bool {
    let out = command(env!("CARGO_BIN_EXE_txtodo"), dir)
        .env("TXTODO_CONFIG", dir.join("none.toml"))
        .args(["--no-id", "--dir", dir.to_str().unwrap(), "-A"])
        .args(step)
        .output()
        .unwrap();
    out.status.success()
}

fn files(dir: &Path) -> Vec<(String, String)> {
    ["todo.txt", "done.txt", "other.txt"]
        .iter()
        .map(|f| {
            (
                f.to_string(),
                String::from_utf8_lossy(&std::fs::read(dir.join(f)).unwrap()).into_owned(),
            )
        })
        .collect()
}

#[test]
fn every_scenario_leaves_byte_identical_files() {
    assert!(
        SCENARIOS.len() >= 25,
        "plan M2 asks for at least 25 scenarios"
    );
    for sc in SCENARIOS {
        let (a, b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        seed(a.path(), sc);
        seed(b.path(), sc);
        for (i, step) in sc.steps.iter().enumerate() {
            let (ok_sh, ok_tx) = (run_todo_sh(a.path(), step), run_txtodo(b.path(), step));
            assert_eq!(
                ok_sh, ok_tx,
                "{}: step {i} {step:?} exit status (todo.sh vs txtodo)",
                sc.name
            );
        }
        assert_eq!(
            files(a.path()),
            files(b.path()),
            "{}: files after {:?}",
            sc.name,
            sc.steps
        );
    }
}
