//! The unified diff a dry-run `Apply` returns: headers, hunks, context, and the coarse fallback.

use crate::unified_diff::unified_diff;

fn diff(old: &str, new: &str) -> String {
    unified_diff("todo.txt", old.as_bytes(), new.as_bytes())
}

#[test]
fn identical_documents_have_an_empty_diff() {
    assert_eq!(diff("a\nb\n", "a\nb\n"), "");
}

#[test]
fn a_changed_line_is_one_hunk_with_the_path_in_both_headers() {
    let d = diff("one\ntwo\nthree\n", "one\nTWO\nthree\n");
    assert_eq!(
        d,
        "--- a/todo.txt\n+++ b/todo.txt\n@@ -1,3 +1,3 @@\n one\n-two\n+TWO\n three\n"
    );
}

#[test]
fn an_appended_line_and_a_removed_line() {
    assert_eq!(
        diff("a\nb\n", "a\nb\nc\n"),
        "--- a/todo.txt\n+++ b/todo.txt\n@@ -1,2 +1,3 @@\n a\n b\n+c\n"
    );
    assert_eq!(
        diff("a\nb\nc\n", "a\nc\n"),
        "--- a/todo.txt\n+++ b/todo.txt\n@@ -1,3 +1,2 @@\n a\n-b\n c\n"
    );
}

#[test]
fn the_top_of_an_empty_file_is_numbered_zero() {
    assert_eq!(
        diff("", "first\n"),
        "--- a/todo.txt\n+++ b/todo.txt\n@@ -0,0 +1,1 @@\n+first\n"
    );
}

#[test]
fn a_moved_line_is_a_delete_and_an_insert() {
    let d = diff("a\nb\nc\n", "b\nc\na\n");
    assert!(d.contains("-a\n") && d.contains("+a\n"), "{d}");
}

#[test]
fn far_apart_changes_are_separate_hunks_and_near_ones_merge() {
    let old: String = (1..=20).map(|n| format!("l{n}\n")).collect();
    let far = old.replace("l2\n", "L2\n").replace("l19\n", "L19\n");
    assert_eq!(diff(&old, &far).matches("@@ -").count(), 2);
    let near = old.replace("l2\n", "L2\n").replace("l6\n", "L6\n");
    assert_eq!(diff(&old, &near).matches("@@ -").count(), 1);
}

#[test]
fn a_change_only_in_the_final_newline_is_named_not_hidden() {
    let d = diff("a\nb", "a\nb\n");
    assert!(d.starts_with("--- a/todo.txt\n+++ b/todo.txt\n"), "{d}");
    assert!(d.contains("final newline"), "{d}");
}

#[test]
fn a_huge_middle_falls_back_to_one_replacement_and_still_applies() {
    let old: String = (0..2_500).map(|n| format!("old{n}\n")).collect();
    let new: String = (0..2_500).map(|n| format!("new{n}\n")).collect();
    let d = diff(&old, &new);
    assert_eq!(d.matches("\n-old").count(), 2_500);
    assert_eq!(d.matches("\n+new").count(), 2_500);
}
