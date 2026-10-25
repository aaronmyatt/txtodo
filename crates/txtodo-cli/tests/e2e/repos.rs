//! `txtodo repos` and the doctor rows end to end (task `two-kinds-of-workspace`, ADR 0043): a real
//! `txtodod` in true global mode, hermetic via `$TXTODO_SOCKET` (so it seeds no `$HOME` root and
//! scans nothing on its own). A scan root is added, a clone with a todo list is found and
//! registered, `doctor` warns that the clone has no merge driver, and `git setup` clears it.
// Integration tests are tests: clippy.toml allows unwrap/expect in #[test] fns but not in their helpers.
#![allow(clippy::expect_used, clippy::unwrap_used)]

use crate::lists::Fixture;
use crate::workspace::stdout;

use std::path::Path;
use std::process::Command;

fn stderr(o: &std::process::Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

/// A clone at `rel` under `home` with a `todo.txt` (a bare `git init`: no commit needed).
fn clone_at(home: &Path, rel: &str) -> std::path::PathBuf {
    let dir = home.join(rel);
    std::fs::create_dir_all(&dir).unwrap();
    let init = Command::new("git")
        .args(["init", "-q"])
        .current_dir(&dir)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .output()
        .unwrap();
    assert!(init.status.success());
    std::fs::write(dir.join("todo.txt"), "a task\n").unwrap();
    dir.canonicalize().unwrap()
}

/// `txtodo git setup` in the clone, with git's own environment scrubbed.
fn git_setup(repo: &Path) {
    let setup = Command::new(env!("CARGO_BIN_EXE_txtodo"))
        .args(["git", "setup"])
        .current_dir(repo)
        .env_remove("GIT_DIR")
        .env_remove("GIT_INDEX_FILE")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .output()
        .unwrap();
    assert!(
        setup.status.success(),
        "{}",
        String::from_utf8_lossy(&setup.stderr)
    );
}

/// `repos` lists the clone and git has been told to ignore `.txtodo/` in it.
fn assert_listed_and_excluded(f: &Fixture, repo: &Path) {
    let listed = stdout(&f.run(&["repos"]));
    assert!(listed.contains(&repo.display().to_string()), "{listed}");
    assert!(
        listed.contains("1 repo(s)") && listed.contains("last scan"),
        "{listed}"
    );
    let exclude = std::fs::read_to_string(repo.join(".git/info/exclude")).unwrap();
    assert!(
        exclude.lines().any(|l| l == ".txtodo/"),
        "kept out of git: {exclude}"
    );
}

#[test]
fn a_scan_root_is_added_and_a_clone_under_it_is_found_registered_and_listed() {
    if Command::new("git").arg("--version").output().is_err() {
        return;
    }
    let f = Fixture::new();
    let home = tempfile::tempdir().unwrap();
    let repo = clone_at(home.path(), "code/a");

    // An isolated daemon seeds no root, so it looks nowhere on its own.
    assert!(stdout(&f.run(&["repos", "roots"])).contains("no scan roots"));
    let root = home.path().to_str().unwrap();
    let added = f.run(&["repos", "roots", "add", root]);
    assert!(added.status.success(), "{}", stderr(&added));
    assert!(stdout(&added).contains(root), "{}", stdout(&added));

    let scanned = f.run(&["repos", "scan"]);
    assert!(scanned.status.success(), "{}", stderr(&scanned));
    assert!(
        stdout(&scanned).contains("found 1 repo(s), registered 1 new"),
        "{}",
        stdout(&scanned)
    );
    assert_listed_and_excluded(&f, &repo);
}

#[test]
fn a_scan_root_comes_off_and_none_left_means_the_daemon_looks_nowhere() {
    let f = Fixture::new();
    let home = tempfile::tempdir().unwrap();
    let root = home.path().to_str().unwrap();
    f.run(&["repos", "roots", "add", root]);
    assert!(stdout(&f.run(&["repos", "roots"])).contains(root));
    let removed = f.run(&["repos", "roots", "rm", root]);
    assert!(removed.status.success(), "{}", stderr(&removed));
    assert!(
        stdout(&removed).contains("no scan roots"),
        "{}",
        stdout(&removed)
    );
}

#[test]
fn doctor_counts_the_repos_and_asks_for_git_setup_until_it_is_run() {
    if Command::new("git").arg("--version").output().is_err() {
        return;
    }
    let f = Fixture::new();
    let home = tempfile::tempdir().unwrap();
    let repo = clone_at(home.path(), "code/a");
    let added = f.run(&["repos", "add", repo.to_str().unwrap()]);
    assert!(added.status.success(), "{}", stderr(&added));

    let doctor = stdout(&f.run(&["doctor"]));
    assert!(
        doctor.contains("repos") && doctor.contains("1 repo workspace(s)"),
        "{doctor}"
    );
    assert!(doctor.contains("txtodo git setup --dir"), "{doctor}");

    git_setup(&repo);
    let doctor = stdout(&f.run(&["doctor"]));
    assert!(
        !doctor.contains("txtodo git setup --dir"),
        "set up: no warning\n{doctor}"
    );
}

#[test]
fn add_registers_a_clone_and_refuses_a_folder_that_is_not_one() {
    if Command::new("git").arg("--version").output().is_err() {
        return;
    }
    let f = Fixture::new();
    let home = tempfile::tempdir().unwrap();
    let repo = clone_at(home.path(), "elsewhere");
    let added = f.run(&["repos", "add", repo.to_str().unwrap()]);
    assert!(added.status.success(), "{}", stderr(&added));
    assert!(stdout(&added).contains("registered"), "{}", stdout(&added));
    assert!(stdout(&f.run(&["repos"])).contains(&repo.display().to_string()));

    let plain = tempfile::tempdir().unwrap();
    let refused = f.run(&["repos", "add", plain.path().to_str().unwrap()]);
    assert!(!refused.status.success());
    assert!(
        stderr(&refused).contains("not a git clone"),
        "{}",
        stderr(&refused)
    );
}
