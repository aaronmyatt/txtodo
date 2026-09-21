//! `txtodo.toml` parsing and the initial layout of a workspace.

use crate::layout_file::{LAYOUT_FILE, LayoutFile, initial, parse, read};
use txtodo_model::WorkspaceLayout;

#[test]
fn both_keys_are_optional_and_the_defaults_fill_the_rest() {
    assert_eq!(parse("").unwrap(), WorkspaceLayout::default());
    assert_eq!(
        parse("refs_dir = \"notes/refs\"\n").unwrap().refs_dir(),
        "notes/refs"
    );
    assert_eq!(
        parse("refs_dir = \".\"\n").unwrap(),
        WorkspaceLayout::beside_the_list()
    );
    assert_eq!(
        parse("todo_file = \"todo.txt\"").unwrap(),
        WorkspaceLayout::default()
    );
}

#[test]
fn unknown_keys_are_ignored_so_a_newer_file_still_loads() {
    assert!(parse("refs_dir = \"tasks\"\nfuture_key = 3\n").is_ok());
}

#[test]
fn bad_toml_and_unsafe_paths_are_refused_with_the_file_named() {
    for bad in [
        "refs_dir = ",
        "refs_dir = \"../up\"",
        "refs_dir = \"/abs\"",
        "refs_dir = 3",
    ] {
        let e = parse(bad).unwrap_err();
        assert!(e.starts_with(LAYOUT_FILE), "{bad}: {e}");
    }
}

#[test]
fn a_different_todo_file_is_refused_until_the_daemon_honours_it() {
    let e = parse("todo_file = \"work.txt\"").unwrap_err();
    assert!(e.contains("not supported yet"), "{e}");
}

#[test]
fn a_missing_file_means_the_defaults_and_a_bad_one_does_too_at_first_open() {
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(read(dir.path()), LayoutFile::Missing);
    assert_eq!(initial(dir.path()), WorkspaceLayout::default());
    std::fs::write(dir.path().join(LAYOUT_FILE), "refs_dir = \".\"\n").unwrap();
    assert_eq!(initial(dir.path()), WorkspaceLayout::beside_the_list());
    std::fs::write(dir.path().join(LAYOUT_FILE), "refs_dir = [").unwrap();
    assert!(matches!(read(dir.path()), LayoutFile::Invalid(_)));
    assert_eq!(initial(dir.path()), WorkspaceLayout::default());
}
