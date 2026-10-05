//! `app_detail.rs`'s pure parts.

use super::*;

fn level(raw: &str, dir: &str) -> Level {
    Level {
        parent: Parent {
            raw: raw.to_owned(),
            ..Parent::default()
        },
        dir: dir.to_owned(),
        ..Level::default()
    }
}

#[test]
fn a_level_is_stale_once_its_parent_names_another_folder() {
    let mut state = AppState::fixture();
    state.detail.levels = vec![
        level("plan ref:trip", "tasks/trip"),
        level("pack ref:bags-2", "tasks/trip/bags"),
        level("no tag yet", "tasks/no-tag-yet"),
    ];
    assert_eq!(stale_levels(&state), vec![1], "only the renamed one");
}
