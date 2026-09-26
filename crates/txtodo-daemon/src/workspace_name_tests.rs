//! `workspace_name.rs` (task workspace-vanity-name): what counts as a name, the one-line edit of
//! `txtodo.toml`, and which name a workspace shows.

use crate::workspace_name::{
    MAX_NAME_BYTES, clean, display_name, offered_name, parse, with_name, without_name,
};
use std::path::Path;
use txtodo_model::Ulid;
use txtodo_store::WorkspaceId;

fn id() -> WorkspaceId {
    WorkspaceId::new(Ulid::from_u128(0x0199_0000_0000_0000_0000_0000_0000_0042))
}

#[test]
fn a_name_is_trimmed_and_an_empty_one_is_none() {
    assert_eq!(
        clean("  Groceries \u{1f955} "),
        Ok(Some("Groceries \u{1f955}".into()))
    );
    assert_eq!(clean("   "), Ok(None));
    assert_eq!(clean(""), Ok(None));
}

#[test]
fn a_control_character_or_an_overlong_name_is_refused() {
    assert!(clean("two\nlines").unwrap_err().contains("new line"));
    assert!(clean("a\ttab").is_err());
    let long = "x".repeat(MAX_NAME_BYTES + 1);
    assert!(clean(&long).unwrap_err().contains("at most"));
    assert_eq!(
        clean(&"x".repeat(MAX_NAME_BYTES)).map(|n| n.map(|n| n.len())),
        Ok(Some(256))
    );
}

#[test]
fn a_new_name_goes_at_the_end_and_every_other_byte_stays() {
    let layout = "# comment\nrefs_dir = \"tasks\"\ntodo_file = \"todo.txt\"\n";
    let out = with_name(layout, Some("Groceries")).unwrap();
    assert_eq!(out, format!("{layout}name = \"Groceries\"\n"));
    assert_eq!(parse(&out).as_deref(), Some("Groceries"));
    // No final new line: one is added before the name, nothing else changes.
    let bare = with_name("refs_dir = \"tasks\"", Some("G")).unwrap();
    assert_eq!(bare, "refs_dir = \"tasks\"\nname = \"G\"\n");
    assert_eq!(with_name("", Some("G")).unwrap(), "name = \"G\"\n");
}

#[test]
fn a_name_is_replaced_where_it_stands_and_removed_for_none() {
    let text = "refs_dir = \"tasks\"\n  name = \"Old\"  # set by hand\ntodo_file = \"todo.txt\"\n";
    let renamed = with_name(text, Some("New")).unwrap();
    assert_eq!(
        renamed,
        "refs_dir = \"tasks\"\nname = \"New\"\ntodo_file = \"todo.txt\"\n"
    );
    let cleared = with_name(text, None).unwrap();
    assert_eq!(cleared, "refs_dir = \"tasks\"\ntodo_file = \"todo.txt\"\n");
    assert_eq!(parse(&cleared), None);
    // `names` and `name_x` are other keys.
    let other = "names = 1\nname_x = 2\n";
    assert_eq!(
        with_name(other, Some("N")).unwrap(),
        format!("{other}name = \"N\"\n")
    );
}

#[test]
fn a_new_name_goes_before_the_first_table() {
    let text = "refs_dir = \"tasks\"\n[future]\nname = \"a table's own key\"\n";
    let out = with_name(text, Some("Top")).unwrap();
    assert_eq!(
        out,
        "refs_dir = \"tasks\"\nname = \"Top\"\n[future]\nname = \"a table's own key\"\n"
    );
    assert_eq!(parse(&out).as_deref(), Some("Top"));
}

#[test]
fn quotes_and_backslashes_are_escaped_and_read_back() {
    let name = r#"Mum's "big" list \ 2026"#;
    let out = with_name("", Some(name)).unwrap();
    assert_eq!(parse(&out).as_deref(), Some(name), "{out}");
}

#[test]
fn a_file_that_is_not_toml_or_hides_the_key_is_refused_untouched() {
    assert!(
        with_name("refs_dir = \n", Some("N"))
            .unwrap_err()
            .contains("not valid TOML")
    );
    let quoted_key = "\"name\" = \"Old\"\n";
    assert!(with_name(quoted_key, Some("N")).is_err());
    assert!(with_name(quoted_key, None).unwrap_err().contains("by hand"));
}

#[test]
fn a_bad_name_in_the_file_reads_as_none() {
    assert_eq!(parse("name = \"\"\n"), None);
    assert_eq!(parse("name = 5\n"), None);
    assert_eq!(parse("name = \"a\\tb\"\n"), None);
    assert_eq!(parse("not toml ="), None);
    assert_eq!(parse("refs_dir = \"tasks\"\n"), None);
}

/// What two names set at once on two devices merge into: two whole lines, which TOML refuses.
const TWO_NAMES: &str = "refs_dir = \"notes\"\nname = \"Groceries\"\nname = \"Shopping\"\n";

#[test]
fn two_name_lines_read_as_the_last_and_the_layout_still_loads() {
    assert_eq!(parse(TWO_NAMES).as_deref(), Some("Shopping"));
    let layout = crate::layout_file::parse(TWO_NAMES).unwrap();
    assert_eq!(layout.refs_dir(), "notes");
    assert_eq!(without_name(TWO_NAMES), "refs_dir = \"notes\"\n");
    // A table's own `name` is not the workspace's.
    assert_eq!(parse("[t]\nname = \"x\"\n\"bad"), None);
}

#[test]
fn the_next_name_writes_one_line_again() {
    let out = with_name(TWO_NAMES, Some("Food")).unwrap();
    assert_eq!(out, "refs_dir = \"notes\"\nname = \"Food\"\n");
    assert_eq!(parse(&out).as_deref(), Some("Food"));
}

#[test]
fn the_file_name_wins_then_default_then_the_folder() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("plants");
    std::fs::create_dir(&root).unwrap();
    let none = || -> Option<String> { panic!("a named folder never asks for an offered name") };
    assert_eq!(display_name(id(), &root, false, none), "plants");
    assert_eq!(display_name(id(), &root, true, none), "default");
    std::fs::write(root.join("txtodo.toml"), "name = \"House plants\"\n").unwrap();
    assert_eq!(display_name(id(), &root, false, none), "House plants");
    assert_eq!(display_name(id(), &root, true, none), "House plants");
}

#[test]
fn a_folder_named_by_the_id_takes_the_offered_name() {
    let dir = tempfile::tempdir().unwrap();
    let mirror = dir.path().join("remote").join(id().to_string());
    std::fs::create_dir_all(&mirror).unwrap();
    let offered = || Some("Groceries".to_owned());
    assert_eq!(display_name(id(), &mirror, false, offered), "Groceries");
    assert_eq!(
        display_name(id(), &mirror, false, || None),
        id().to_string()
    );
    // The synced file still wins over what a device offered.
    std::fs::write(mirror.join("txtodo.toml"), "name = \"Shop\"\n").unwrap();
    assert_eq!(display_name(id(), &mirror, false, offered), "Shop");
    assert_eq!(display_name(id(), Path::new("/"), false, || None), "/");
}

#[test]
fn an_offered_name_that_is_the_id_says_nothing() {
    assert_eq!(offered_name(id(), &id().to_string()), None);
    assert_eq!(offered_name(id(), " "), None);
    assert_eq!(offered_name(id(), "bad\nname"), None);
    assert_eq!(
        offered_name(id(), " Groceries ").as_deref(),
        Some("Groceries")
    );
}
