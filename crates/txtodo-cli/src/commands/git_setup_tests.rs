//! `git_setup.rs`: the pure plan, then the command in a real temp clone (skipped without `git`).

use super::*;

fn has_git() -> bool {
    Command::new("git").arg("--version").output().is_ok()
}

fn git_in(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        // Run from a pre-commit hook, git exports its own clone's: they must not reach these.
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@example.invalid")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@example.invalid")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_owned()
}

fn clone() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    git_in(dir.path(), &["init", "-q", "-b", "main"]);
    dir
}

#[test]
fn the_lists_get_the_txtodo_merge_and_done_txt_gets_union() {
    assert_eq!(
        wanted_attributes("todo.txt"),
        ["todo.txt merge=txtodo", "done.txt merge=union"]
    );
    assert_eq!(
        wanted_attributes("work.txt"),
        [
            "todo.txt merge=txtodo",
            "work.txt merge=txtodo",
            "done.txt merge=union"
        ]
    );
}

#[test]
fn only_what_is_missing_is_planned() {
    let wanted = wanted_attributes("todo.txt");
    let none = plan("", "", None, &wanted, "txtodo merge-driver %O %A %B");
    assert_eq!(none.attributes, wanted);
    assert!(none.exclude);
    assert_eq!(none.driver.as_deref(), Some("txtodo merge-driver %O %A %B"));

    let done = plan(
        "*.png binary\ntodo.txt merge=txtodo\n# note\ndone.txt merge=union\n",
        "# local\n.txtodo/\n",
        Some("txtodo merge-driver %O %A %B"),
        &wanted,
        "txtodo merge-driver %O %A %B",
    );
    assert!(done.is_empty(), "{done:?}");

    // A different driver command (txtodo moved) is replaced; a half-done file is finished.
    let stale = plan(
        "todo.txt merge=txtodo\n",
        ".txtodo\n",
        Some("/old/txtodo merge-driver %O %A %B"),
        &wanted,
        "txtodo merge-driver %O %A %B",
    );
    assert_eq!(stale.attributes, ["done.txt merge=union"]);
    assert!(!stale.exclude, "`.txtodo` without the slash counts");
    assert!(stale.driver.is_some());
}

#[test]
fn a_path_with_a_space_or_quote_is_quoted_for_the_shell() {
    assert_eq!(
        driver_command(Path::new("/opt/homebrew/bin/txtodo")),
        "/opt/homebrew/bin/txtodo merge-driver %O %A %B"
    );
    assert_eq!(
        driver_command(Path::new("/Users/a b/bin/txtodo")),
        "'/Users/a b/bin/txtodo' merge-driver %O %A %B"
    );
    assert_eq!(
        driver_command(Path::new("/it's/txtodo")),
        r"'/it'\''s/txtodo' merge-driver %O %A %B"
    );
}

#[cfg(unix)]
#[test]
fn the_name_on_path_is_used_when_it_is_this_binary_not_the_resolved_file() {
    let dir = tempfile::tempdir().unwrap();
    let real = dir.path().join("Cellar-1.2.3-txtodo");
    std::fs::write(&real, "bin").unwrap();
    let bin = dir.path().join("bin");
    std::fs::create_dir(&bin).unwrap();
    std::os::unix::fs::symlink(&real, bin.join("txtodo")).unwrap();
    let path = std::env::join_paths([&bin]).unwrap();
    assert_eq!(txtodo_path(Some(&path), &real), bin.join("txtodo"));
    // Another txtodo on PATH is not this one: fall back to the running binary.
    let other = dir.path().join("other-txtodo");
    std::fs::write(&other, "bin2").unwrap();
    assert_eq!(txtodo_path(Some(&path), &other), other);
    assert_eq!(txtodo_path(None, &real), real);
}

#[test]
fn setup_in_a_clone_writes_the_three_things_and_a_second_run_changes_nothing() {
    if !has_git() {
        return;
    }
    let repo = clone();
    let root = repo.path();
    let action = Action::Setup { dry_run: false };
    let dir = root.to_str();

    run(&action, dir).unwrap();

    let attributes = std::fs::read_to_string(root.join(".gitattributes")).unwrap();
    assert!(
        attributes.contains("todo.txt merge=txtodo\n"),
        "{attributes}"
    );
    assert!(
        attributes.contains("done.txt merge=union\n"),
        "{attributes}"
    );
    let exclude = std::fs::read_to_string(root.join(".git/info/exclude")).unwrap();
    assert!(exclude.lines().any(|l| l == ".txtodo/"), "{exclude}");
    let driver = git_in(root, &["config", "--local", "--get", "merge.txtodo.driver"]);
    assert!(driver.ends_with("merge-driver %O %A %B"), "{driver}");

    run(&action, dir).unwrap();
    assert_eq!(
        std::fs::read_to_string(root.join(".gitattributes")).unwrap(),
        attributes
    );
    assert_eq!(
        std::fs::read_to_string(root.join(".git/info/exclude")).unwrap(),
        exclude
    );
}

#[test]
fn a_dry_run_changes_nothing_and_existing_attributes_are_kept() {
    if !has_git() {
        return;
    }
    let repo = clone();
    let root = repo.path();
    std::fs::write(root.join(".gitattributes"), "*.png binary").unwrap();
    run(&Action::Setup { dry_run: true }, root.to_str()).unwrap();
    assert_eq!(
        std::fs::read_to_string(root.join(".gitattributes")).unwrap(),
        "*.png binary"
    );
    let set = Command::new("git")
        .args(["config", "--local", "--get", "merge.txtodo.driver"])
        .current_dir(root)
        .env_remove("GIT_DIR")
        .env_remove("GIT_INDEX_FILE")
        .output()
        .unwrap();
    assert!(!set.status.success(), "dry run set the driver");

    run(&Action::Setup { dry_run: false }, root.to_str()).unwrap();
    let attributes = std::fs::read_to_string(root.join(".gitattributes")).unwrap();
    assert!(attributes.starts_with("*.png binary\n"), "{attributes}");
    assert!(attributes.contains(HEADER), "{attributes}");
}

#[test]
fn a_folder_that_is_no_clone_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    // A temp dir could sit inside a clone on some machines; make sure it does not.
    if repo_root(dir.path()).is_some() {
        return;
    }
    let why = run(&Action::Setup { dry_run: false }, dir.path().to_str())
        .unwrap_err()
        .to_string();
    assert!(why.contains("not inside a git clone"), "{why}");
}

#[test]
fn a_linked_worktree_excludes_through_the_shared_git_dir() {
    if !has_git() {
        return;
    }
    let repo = clone();
    git_in(
        repo.path(),
        &["commit", "-q", "--allow-empty", "-m", "init"],
    );
    let wt = repo.path().parent().unwrap().join(format!(
        "{}-wt",
        repo.path().file_name().unwrap().to_string_lossy()
    ));
    git_in(
        repo.path(),
        &["worktree", "add", "-q", wt.to_str().unwrap(), "-b", "other"],
    );
    run(&Action::Setup { dry_run: false }, wt.to_str()).unwrap();
    let exclude = std::fs::read_to_string(repo.path().join(".git/info/exclude")).unwrap();
    assert!(exclude.lines().any(|l| l == ".txtodo/"), "{exclude}");
    assert!(
        wt.join(".gitattributes").is_file(),
        "the attributes belong to the worktree's files"
    );
    std::fs::remove_dir_all(&wt).ok();
}
