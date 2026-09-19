//! The `r` conflict-review pane: lists `needs_review` flags and maps a `mine`/`theirs`/`merged`
//! pick to a `ResolveConflict` RPC request (design §7, plan M4). Resolution is sent through the
//! daemon's own `ResolveConflict` RPC (`crates/txtodo-proto`'s `ResolveRequest`/`Resolution`)
//! rather than a hand-rolled `Apply { mutations: [Edit] }` — the daemon already exposes exactly
//! this operation (writes the chosen side back and clears the flag, both or neither), so
//! reusing it here avoids a second, possibly-drifting implementation of the same merge rule.

use crossterm::event::{KeyCode, KeyEvent};
use txtodo_proto::v1 as pb;

use crate::state::{AppState, Resolution};

impl From<Resolution> for pb::Resolution {
    fn from(r: Resolution) -> Self {
        match r {
            Resolution::Mine => pb::Resolution::Mine,
            Resolution::Theirs => pb::Resolution::Theirs,
            Resolution::Merged => pb::Resolution::Merged,
        }
    }
}

/// `j`/`k` inside the pane, clamped to the flag list.
pub fn move_down(state: &mut AppState) {
    if state.needs_review.is_empty() {
        return;
    }
    state.conflict_cursor = (state.conflict_cursor + 1).min(state.needs_review.len() - 1);
}

/// `j`/`k` inside the pane, clamped to the flag list.
pub fn move_up(state: &mut AppState) {
    state.conflict_cursor = state.conflict_cursor.saturating_sub(1);
}

/// One keystroke while the pane has focus, other than the mine/theirs/merged picks themselves
/// (`app.rs` maps those to `resolve_request` because only it holds the `Daemon` to send the
/// result through). Returns `true` when the key was handled.
pub fn on_key(state: &mut AppState, key: KeyEvent) -> bool {
    match key.code {
        KeyCode::Char('j') | KeyCode::Down => move_down(state),
        KeyCode::Char('k') | KeyCode::Up => move_up(state),
        _ => return false,
    }
    true
}

/// Builds the `ResolveConflict` request for the currently selected flag, if any. `path` is the
/// document the flag belongs to (`AppState.path`).
pub fn resolve_request(state: &AppState, resolution: Resolution) -> Option<pb::ResolveRequest> {
    let flag = state.needs_review.get(state.conflict_cursor)?;
    Some(pb::ResolveRequest {
        workspace: None,
        path: state.path.clone(),
        task: Some(pb::TaskRef {
            line_number: flag.line_number,
            task_id: flag.task_id.clone(),
        }),
        resolution: pb::Resolution::from(resolution) as i32,
        agent: None, // the human on this device
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::ConflictItem;

    fn two_flags() -> AppState {
        let mut state = AppState::fixture();
        state.needs_review.push(ConflictItem {
            task_id: "01J9K3H5Z7Q8X2M4N6P8R0T2V6".to_owned(),
            line_number: 4,
            mine: "water plants".to_owned(),
            theirs: "water the plants".to_owned(),
        });
        state
    }

    #[test]
    fn navigation_clamps_to_the_flag_list() {
        let mut state = two_flags();
        move_up(&mut state);
        assert_eq!(state.conflict_cursor, 0);
        move_down(&mut state);
        assert_eq!(state.conflict_cursor, 1);
        move_down(&mut state);
        assert_eq!(state.conflict_cursor, 1, "clamped at the last flag");
    }

    #[test]
    fn resolve_request_addresses_the_selected_flags_task() {
        let state = two_flags();
        let req = resolve_request(&state, Resolution::Merged).unwrap();
        assert_eq!(req.path, "todo.txt");
        assert_eq!(req.task.unwrap().line_number, 2);
        assert_eq!(req.resolution, pb::Resolution::Merged as i32);
    }

    #[test]
    fn resolve_request_is_none_with_no_flags() {
        let mut state = AppState::fixture();
        state.needs_review.clear();
        assert!(resolve_request(&state, Resolution::Mine).is_none());
    }

    #[test]
    fn each_resolution_maps_to_its_own_pb_variant() {
        assert_eq!(pb::Resolution::from(Resolution::Mine), pb::Resolution::Mine);
        assert_eq!(
            pb::Resolution::from(Resolution::Theirs),
            pb::Resolution::Theirs
        );
        assert_eq!(
            pb::Resolution::from(Resolution::Merged),
            pb::Resolution::Merged
        );
    }
}
