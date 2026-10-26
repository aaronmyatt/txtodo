//! Unit tests for `list_param.rs`.

use super::*;
use serde_json::json;

fn workspace(id: &str, name: &str, root: &str, is_list: bool) -> WorkspaceInfo {
    WorkspaceInfo {
        id: id.to_owned(),
        root: root.to_owned(),
        added_at_ms: 0,
        root_exists: true,
        has_state: true,
        name: name.to_owned(),
        is_list,
    }
}

fn all() -> Vec<WorkspaceInfo> {
    vec![
        workspace("01D", "default", "/d/default", true),
        workspace("01W", "projects", "/d/lists/work", true),
        workspace("01R", "txtodo", "/home/a/txtodo", false),
    ]
}

#[test]
fn a_name_picks_a_list_by_what_it_shows_or_was_made_under() {
    assert_eq!(pick(&all(), "projects").unwrap(), "01W");
    assert_eq!(pick(&all(), "Work").unwrap(), "01W");
    assert_eq!(pick(&all(), "default").unwrap(), "01D");
}

#[test]
fn a_repo_is_not_a_list_and_the_refusal_names_the_lists() {
    let why = pick(&all(), "txtodo").unwrap_err();
    assert!(why.contains("no list named"), "{why}");
    assert!(why.contains("default, projects"), "{why}");
    let none = pick(&[workspace("01R", "r", "/r", false)], "x").unwrap_err();
    assert!(none.contains("(none)"), "{none}");
}

#[test]
fn the_list_property_is_added_only_where_a_workspace_one_is() {
    let mut with = Tool::new(
        "with",
        "d",
        Arc::new(
            json!({"type": "object", "properties": {"workspace": {"type": "string"}}})
                .as_object()
                .cloned()
                .unwrap(),
        ),
    );
    add_to_schema(&mut with);
    assert!(with.input_schema["properties"]["list"].is_object());
    assert!(with.input_schema["properties"]["workspace"].is_object());

    let mut without = Tool::new(
        "without",
        "d",
        Arc::new(
            json!({"type": "object", "properties": {}})
                .as_object()
                .cloned()
                .unwrap(),
        ),
    );
    add_to_schema(&mut without);
    assert!(without.input_schema["properties"].get("list").is_none());
}
