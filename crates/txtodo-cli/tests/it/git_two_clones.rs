//! Two clones of one repo, each working its todo.txt with `txtodo` in direct-file mode, meeting
//! through a real remote and a real `git pull` (task two-kinds-of-workspace, ADR 0043): every line
//! either side added or completed is there after the merge, and the two clones end with one file.
//! Skipped when `git` is not installed.

// Integration tests are tests: clippy.toml allows unwrap/expect in #[test] fns but not in their helpers.
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::print_stderr)]

use std::path::Path;
use std::process::{Command, Output};

fn git(dir: &Path, args: &[&str]) -> Output {
    Command::new("git")
        .args(args)
        .current_dir(dir)
        // Not what git exports to a pre-commit hook, nor the machine's own git setup.
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@example.invalid")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@example.invalid")
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .expect("git runs")
}

fn ok(dir: &Path, args: &[&str]) -> String {
    let out = git(dir, args);
    assert!(
        out.status.success(),
        "git {args:?} in {}: {}{}",
        dir.display(),
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// `txtodo` in direct-file mode on the clone's own todo.txt.
fn txtodo(clone: &Path, args: &[&str]) {
    let out = Command::new(env!("CARGO_BIN_EXE_txtodo"))
        .arg("--no-daemon")
        .arg("--dir")
        .arg(clone)
        .args(args)
        .env("TXTODO_CONFIG", clone.join("none.toml"))
        .env_remove("TXTODO_TODO_DIR")
        .env("TXTODO_NO_AUTOSTART", "1")
        .output()
        .expect("txtodo runs");
    assert!(
        out.status.success(),
        "txtodo {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn setup(clone: &Path) {
    let out = Command::new(env!("CARGO_BIN_EXE_txtodo"))
        .args(["git", "setup"])
        .current_dir(clone)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .output()
        .expect("txtodo runs");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn read(clone: &Path) -> String {
    std::fs::read_to_string(clone.join("todo.txt")).unwrap()
}

/// A remote and two clones of it holding a three-line list, each with the merge set up.
fn two_clones(root: &Path) -> (std::path::PathBuf, std::path::PathBuf) {
    let origin = root.join("origin.git");
    std::fs::create_dir(&origin).unwrap();
    ok(&origin, &["init", "-q", "--bare", "-b", "main"]);
    let (a, b) = (root.join("a"), root.join("b"));
    ok(root, &["clone", "-q", origin.to_str().unwrap(), "a"]);
    setup(&a);
    for line in ["alpha", "bravo", "charlie"] {
        txtodo(&a, &["add", line]);
    }
    ok(&a, &["add", ".gitattributes", "todo.txt"]);
    ok(&a, &["commit", "-q", "-m", "base"]);
    ok(&a, &["push", "-q", "origin", "HEAD:main"]);
    ok(root, &["clone", "-q", origin.to_str().unwrap(), "b"]);
    setup(&b);
    assert_eq!(read(&a), read(&b), "the same list to start from");
    (a, b)
}

/// Apart: A completes alpha and adds two, pushing; B completes bravo and adds one.
fn work_apart(a: &Path, b: &Path) {
    txtodo(a, &["do", "1"]);
    txtodo(a, &["add", "from a one"]);
    txtodo(a, &["add", "from a two"]);
    ok(a, &["commit", "-q", "-am", "a works"]);
    ok(a, &["push", "-q", "origin", "HEAD:main"]);
    txtodo(b, &["do", "2"]);
    txtodo(b, &["add", "from b one"]);
    ok(b, &["commit", "-q", "-am", "b works"]);
}

/// Every added line is in the merged text; each completed one is there once, done, not open.
fn assert_nothing_lost(merged: &str) {
    assert!(!merged.contains("<<<<<<<"), "no conflict markers: {merged}");
    let lines: Vec<&str> = merged.lines().collect();
    for want in ["from a one", "from a two", "from b one", "charlie"] {
        assert!(
            lines.iter().any(|l| l.contains(want)),
            "{want:?} lost from {merged}"
        );
    }
    let count = |name: &str, done: bool| {
        let of = |l: &&&str| l.contains(name) && l.starts_with("x ") == done;
        lines.iter().filter(of).count()
    };
    for name in ["alpha", "bravo"] {
        assert_eq!(
            (count(name, true), count(name, false)),
            (1, 0),
            "{name}: {merged}"
        );
    }
}

#[test]
fn lines_added_and_completed_in_two_clones_all_survive_a_pull() {
    if Command::new("git").arg("--version").output().is_err() {
        eprintln!("git is not installed; skipping");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let (a, b) = two_clones(dir.path());
    work_apart(&a, &b);

    // B pulls: both changed the file, and the driver merges it.
    ok(
        &b,
        &["pull", "-q", "--no-rebase", "--no-edit", "origin", "main"],
    );
    assert_nothing_lost(&read(&b));

    // B pushes, A pulls: one file in both clones.
    ok(&b, &["push", "-q", "origin", "HEAD:main"]);
    ok(
        &a,
        &["pull", "-q", "--no-rebase", "--no-edit", "origin", "main"],
    );
    assert_eq!(read(&a), read(&b), "both clones end with the same file");
}
