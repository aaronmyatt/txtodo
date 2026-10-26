//! Unit tests for `list_pick.rs`.

use super::*;

fn lists<'a>(rows: &'a [(&'a str, &'a str)]) -> Vec<ListRef<'a>> {
    rows.iter()
        .map(|(shown, root)| ListRef {
            shown,
            root: Path::new(root),
        })
        .collect()
}

#[test]
fn a_name_finds_the_list_that_shows_it_whatever_the_case() {
    let rows = [("work", "/d/lists/work"), ("home", "/d/lists/home")];
    let l = lists(&rows);
    assert_eq!(pick_list(&l, "home"), Pick::Found(1));
    assert_eq!(pick_list(&l, "  HOME "), Pick::Found(1));
}

#[test]
fn a_renamed_list_answers_to_both_names() {
    let rows = [("projects", "/d/lists/work"), ("home", "/d/lists/home")];
    let l = lists(&rows);
    assert_eq!(pick_list(&l, "projects"), Pick::Found(0), "what it shows");
    assert_eq!(
        pick_list(&l, "work"),
        Pick::Found(0),
        "what it was made under"
    );
}

#[test]
fn what_a_list_shows_beats_what_another_was_made_under() {
    // `home` was renamed `work`; a second list was made under `work` and shows `jobs`.
    let rows = [("work", "/d/lists/home"), ("jobs", "/d/lists/work")];
    assert_eq!(pick_list(&lists(&rows), "work"), Pick::Found(0));
}

#[test]
fn two_lists_showing_one_name_are_ambiguous_but_the_made_under_name_reaches_either() {
    let rows = [
        ("x", "/d/lists/a"),
        ("x", "/d/lists/b"),
        ("y", "/d/lists/c"),
    ];
    let l = lists(&rows);
    assert_eq!(pick_list(&l, "x"), Pick::Ambiguous(vec![0, 1]));
    assert_eq!(pick_list(&l, "b"), Pick::Found(1));
}

#[test]
fn an_unknown_name_is_not_found() {
    let rows = [("work", "/d/lists/work")];
    assert_eq!(pick_list(&lists(&rows), "nope"), Pick::NotFound);
    assert_eq!(pick_list(&[], "work"), Pick::NotFound);
}
