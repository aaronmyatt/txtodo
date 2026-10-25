//! `repo_git.rs` on real temp clones, and (when `git` is installed) git itself reading what it wrote.

use super::*;
use std::process::Command;

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_default()
}

/// A folder with an empty `.git` folder, which is all the daemon needs to see.
fn clone_dir() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join(".git")).unwrap();
    dir
}

#[test]
fn a_fresh_clone_gets_all_three_once_and_the_second_call_writes_nothing() {
    let dir = clone_dir();
    let git = dir.path().join(".git");
    let first = prepare(dir.path()).unwrap();
    assert_eq!(
        first,
        Prepared {
            exclude: true,
            attributes: true,
            driver: true
        }
    );
    assert_eq!(read(&git.join("info/exclude")), ".txtodo/\n");
    let attributes = read(&git.join("info/attributes"));
    assert!(
        attributes.contains("todo.txt merge=txtodo\n"),
        "{attributes}"
    );
    assert!(
        attributes.contains("done.txt merge=union\n"),
        "{attributes}"
    );
    let config = read(&git.join("config"));
    assert!(config.contains("[merge \"txtodo\"]"), "{config}");
    assert!(
        config.contains("driver = txtodo merge-driver %O %A %B"),
        "{config}"
    );

    assert!(!prepare(dir.path()).unwrap().any(), "nothing left to write");
    assert_eq!(read(&git.join("info/attributes")), attributes);
    assert_eq!(read(&git.join("config")), config);
}

#[test]
fn a_folder_with_no_git_is_untouched() {
    let dir = tempfile::tempdir().unwrap();
    assert!(!prepare(dir.path()).unwrap().any());
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
}

#[test]
fn what_the_person_or_git_setup_already_set_is_kept() {
    let dir = clone_dir();
    let git = dir.path().join(".git");
    std::fs::write(
        git.join("config"),
        "[core]\n\tbare = false\n[MERGE \"txtodo\"]\n\tdriver = /opt/bin/txtodo merge-driver %O %A %B",
    )
    .unwrap();
    std::fs::write(dir.path().join(".gitattributes"), "todo.txt merge=txtodo\n").unwrap();
    std::fs::create_dir(git.join("info")).unwrap();
    std::fs::write(git.join("info/attributes"), "*.png binary").unwrap();

    let done = prepare(dir.path()).unwrap();
    assert!(done.exclude && done.attributes, "{done:?}");
    assert!(!done.driver, "a driver already set is never replaced");
    assert!(
        read(&git.join("config")).ends_with("%O %A %B"),
        "config untouched"
    );
    let attributes = read(&git.join("info/attributes"));
    assert!(attributes.starts_with("*.png binary\n"), "{attributes}");
    assert!(attributes.contains("done.txt merge=union"), "{attributes}");
    assert!(
        !attributes.contains("todo.txt merge=txtodo"),
        "the committed .gitattributes already has it: {attributes}"
    );
}

#[test]
fn a_merge_section_without_a_driver_gets_one_and_another_subsection_does_not_count() {
    assert!(!has_driver("[merge \"other\"]\n\tdriver = x\n"));
    assert!(!has_driver("[merge \"txtodo\"]\n\tname = n\n"));
    assert!(has_driver("[merge \"txtodo\"]\n\tname = n\n\tdriver = x\n"));
    assert!(!has_driver("[merge \"txtodo\"]\n[core]\n\tdriver = x\n"));
    let dir = clone_dir();
    std::fs::write(
        dir.path().join(".git/config"),
        "[merge \"txtodo\"]\n\tname = n\n",
    )
    .unwrap();
    assert!(prepare(dir.path()).unwrap().driver);
}

#[test]
fn a_root_list_txtodo_toml_names_gets_its_own_attribute() {
    let dir = clone_dir();
    std::fs::write(
        dir.path().join("txtodo.toml"),
        "todo_file = \"tasks.txt\"\n",
    )
    .unwrap();
    prepare(dir.path()).unwrap();
    let attributes = read(&dir.path().join(".git/info/attributes"));
    assert!(
        attributes.contains("tasks.txt merge=txtodo\n"),
        "{attributes}"
    );
}

#[test]
fn a_clone_git_is_writing_to_is_left_for_the_next_scan() {
    let dir = clone_dir();
    std::fs::write(dir.path().join(".git/config.lock"), "").unwrap();
    assert!(!prepare(dir.path()).unwrap().driver);
    assert!(!dir.path().join(".git/config").exists());
}

#[test]
fn a_linked_worktree_writes_the_shared_git_folder() {
    let dir = tempfile::tempdir().unwrap();
    let main = dir.path().join("main");
    let admin = main.join(".git/worktrees/wt");
    std::fs::create_dir_all(&admin).unwrap();
    std::fs::write(admin.join("commondir"), "../..\n").unwrap();
    let wt = dir.path().join("wt");
    std::fs::create_dir(&wt).unwrap();
    std::fs::write(wt.join(".git"), format!("gitdir: {}\n", admin.display())).unwrap();
    assert!(prepare(&wt).unwrap().driver);
    assert!(read(&main.join(".git/config")).contains("[merge \"txtodo\"]"));
    assert!(main.join(".git/info/attributes").is_file());
}

/// The files parse the way git reads them: `git config` sees the driver, `git check-attr` the merge
/// attribute. Skipped without `git`.
#[test]
fn git_reads_what_was_written() {
    let dir = tempfile::tempdir().unwrap();
    let git = |args: &[&str]| {
        Command::new("git")
            .arg("-C")
            .arg(dir.path())
            .args(args)
            // Git exports these to a hook it runs; they would point this at that clone.
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_INDEX_FILE")
            .output()
    };
    if git(&["init", "-q"]).map_or(true, |o| !o.status.success()) {
        return;
    }
    prepare(dir.path()).unwrap();
    let driver = git(&["config", "--get", "merge.txtodo.driver"]).unwrap();
    assert_eq!(
        String::from_utf8_lossy(&driver.stdout).trim(),
        DRIVER,
        "{driver:?}"
    );
    let attr = git(&["check-attr", "merge", "--", "tasks/x/todo.txt"]).unwrap();
    assert!(
        String::from_utf8_lossy(&attr.stdout).contains("merge: txtodo"),
        "{attr:?}"
    );
    let done = git(&["check-attr", "merge", "--", "done.txt"]).unwrap();
    assert!(
        String::from_utf8_lossy(&done.stdout).contains("merge: union"),
        "{done:?}"
    );
}
