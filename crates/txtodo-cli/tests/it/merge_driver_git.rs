//! A real `git merge` of a todo.txt through `txtodo merge-driver` (task two-kinds-of-workspace): both
//! branches edit the same line and add at the end, the case git's own text merge stops on, and the
//! merge finishes with every line there. Skipped when `git` is not installed.
//! Ref: <https://git-scm.com/docs/gitattributes#_defining_a_custom_merge_driver>

// Integration tests are tests: clippy.toml allows unwrap/expect in #[test] fns but not in their helpers.
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::print_stderr)]

use std::path::Path;
use std::process::{Command, Output};

fn git(dir: &Path, args: &[&str]) -> Output {
    Command::new("git")
        .args(args)
        .current_dir(dir)
        // Nothing from the machine's own git setup: no global config, no signing, no hooks.
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@example.invalid")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@example.invalid")
        .output()
        .expect("git runs")
}

fn ok(dir: &Path, args: &[&str]) {
    let out = git(dir, args);
    assert!(
        out.status.success(),
        "git {args:?}: {}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}

fn commit_list(dir: &Path, text: &str, message: &str) {
    std::fs::write(dir.join("todo.txt"), text).unwrap();
    ok(dir, &["add", "todo.txt"]);
    ok(dir, &["commit", "-q", "-m", message]);
}

#[test]
fn git_merges_a_todo_list_through_the_driver_and_keeps_every_line() {
    if Command::new("git").arg("--version").output().is_err() {
        eprintln!("git is not installed; skipping");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();
    ok(repo, &["init", "-q", "-b", "main"]);
    std::fs::write(repo.join(".gitattributes"), "todo.txt merge=txtodo\n").unwrap();
    ok(repo, &["add", ".gitattributes"]);
    let driver = format!("'{}' merge-driver %O %A %B", env!("CARGO_BIN_EXE_txtodo"));
    ok(repo, &["config", "merge.txtodo.driver", &driver]);
    commit_list(repo, "call mum\nbuy milk\nold chore\n", "base");

    ok(repo, &["checkout", "-q", "-b", "other"]);
    commit_list(
        repo,
        "call mum @phone\nbuy milk\nold chore\nfrom other\n",
        "other",
    );
    ok(repo, &["checkout", "-q", "main"]);
    commit_list(repo, "call mum +family\nbuy milk\nfrom main\n", "main");

    let merge = git(repo, &["merge", "other", "-m", "merge other"]);
    assert!(
        merge.status.success(),
        "the driver should settle it: {}{}",
        String::from_utf8_lossy(&merge.stdout),
        String::from_utf8_lossy(&merge.stderr)
    );
    let merged = std::fs::read_to_string(repo.join("todo.txt")).unwrap();
    let lines: Vec<&str> = merged.lines().collect();
    for want in [
        "call mum +family",
        "call mum @phone",
        "buy milk",
        "from main",
        "from other",
    ] {
        assert!(lines.contains(&want), "{want:?} missing from {lines:?}");
    }
    assert!(!lines.contains(&"old chore"), "main deleted it: {lines:?}");
    assert!(!lines.contains(&"call mum"), "both edited it: {lines:?}");
    assert!(!merged.contains("<<<<<<<"), "no conflict markers: {merged}");
}

#[test]
fn without_the_driver_the_same_merge_stops_on_a_conflict() {
    if Command::new("git").arg("--version").output().is_err() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();
    ok(repo, &["init", "-q", "-b", "main"]);
    commit_list(repo, "call mum\nbuy milk\n", "base");
    ok(repo, &["checkout", "-q", "-b", "other"]);
    commit_list(repo, "call mum @phone\nbuy milk\nfrom other\n", "other");
    ok(repo, &["checkout", "-q", "main"]);
    commit_list(repo, "call mum +family\nbuy milk\nfrom main\n", "main");
    let merge = git(repo, &["merge", "other", "-m", "merge other"]);
    assert!(!merge.status.success(), "git's own merge conflicts here");
}
