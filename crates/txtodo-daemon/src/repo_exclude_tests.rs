//! `repo_exclude.rs` against the three layouts git has.

use super::*;

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap()
}

#[test]
fn a_clone_gets_the_line_once_and_keeps_what_was_there() {
    let dir = tempfile::tempdir().unwrap();
    let exclude = dir.path().join(".git/info/exclude");
    std::fs::create_dir_all(exclude.parent().unwrap()).unwrap();
    std::fs::write(&exclude, "# mine\n*.swp").unwrap(); // no trailing newline

    assert!(exclude_txtodo(dir.path()).unwrap());
    assert_eq!(read(&exclude), "# mine\n*.swp\n.txtodo/\n");
    assert!(!exclude_txtodo(dir.path()).unwrap(), "already there");
    assert_eq!(read(&exclude), "# mine\n*.swp\n.txtodo/\n");
}

#[test]
fn a_missing_info_folder_is_made() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join(".git")).unwrap();
    assert!(exclude_txtodo(dir.path()).unwrap());
    assert_eq!(read(&dir.path().join(".git/info/exclude")), ".txtodo/\n");
}

#[test]
fn the_same_line_without_the_slash_counts() {
    let dir = tempfile::tempdir().unwrap();
    let exclude = dir.path().join(".git/info/exclude");
    std::fs::create_dir_all(exclude.parent().unwrap()).unwrap();
    std::fs::write(&exclude, ".txtodo\n").unwrap();
    assert!(!exclude_txtodo(dir.path()).unwrap());
}

#[test]
fn a_linked_worktree_writes_the_shared_git_folder_through_commondir() {
    let dir = tempfile::tempdir().unwrap();
    let main = dir.path().join("main");
    let worktrees = main.join(".git/worktrees/wt");
    std::fs::create_dir_all(&worktrees).unwrap();
    std::fs::write(worktrees.join("commondir"), "../..\n").unwrap();
    let wt = dir.path().join("wt");
    std::fs::create_dir(&wt).unwrap();
    std::fs::write(
        wt.join(".git"),
        format!("gitdir: {}\n", worktrees.display()),
    )
    .unwrap();

    assert_eq!(
        info_exclude(&wt).unwrap(),
        worktrees.join("../..").join("info/exclude")
    );
    assert!(exclude_txtodo(&wt).unwrap());
    assert_eq!(read(&main.join(".git/info/exclude")), ".txtodo/\n");
}

#[test]
fn a_submodule_writes_its_own_git_folder_and_a_relative_gitdir_is_followed() {
    let dir = tempfile::tempdir().unwrap();
    let module = dir.path().join("super/.git/modules/sub");
    std::fs::create_dir_all(&module).unwrap();
    let sub = dir.path().join("super/sub");
    std::fs::create_dir_all(&sub).unwrap();
    std::fs::write(sub.join(".git"), "gitdir: ../.git/modules/sub\n").unwrap();
    assert!(exclude_txtodo(&sub).unwrap());
    assert_eq!(read(&module.join("info/exclude")), ".txtodo/\n");
}

#[test]
fn a_folder_with_no_git_is_left_alone() {
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(info_exclude(dir.path()), None);
    assert!(!exclude_txtodo(dir.path()).unwrap());
    assert!(!dir.path().join(".git").exists());
}
