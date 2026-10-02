//! The `r` conflict-review pane: lists `needs_review` flags and maps a `mine`/`theirs`/`merged`
//! pick to a `ResolveConflict` RPC request (design §7, plan M4). Resolution is sent through the
//! daemon's own `ResolveConflict` RPC (`crates/txtodo-proto`'s `ResolveRequest`/`Resolution`)
//! rather than a hand-rolled `Apply { mutations: [Edit] }` — the daemon already exposes exactly
//! this operation (writes the chosen side back and clears the flag, both or neither), so
//! reusing it here avoids a second, possibly-drifting implementation of the same merge rule.
//!
//! After the flags the pane walks the file's duplicate groups (ADR 0032): two or more lines that
//! read the same. `n`/`o` keep the newest or the oldest copy and delete the rest with an ordinary
//! `Apply` `Delete` by task id, as `txtodo conflicts` does; there is no resolve RPC for them.

use txtodo_proto::v1 as pb;

use crate::state::{AppState, DuplicateGroup, Resolution};

/// Which copy of a duplicate group survives `keep_request`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Keep {
    /// The newest task id (the last copy): the safe default (ADR 0032).
    Newest,
    /// The oldest task id (the first copy).
    Oldest,
}

impl From<Resolution> for pb::Resolution {
    fn from(r: Resolution) -> Self {
        match r {
            Resolution::Mine => pb::Resolution::Mine,
            Resolution::Theirs => pb::Resolution::Theirs,
            Resolution::Merged => pb::Resolution::Merged,
        }
    }
}

/// `j`/`k` inside the pane, clamped to the flags and groups.
pub fn move_down(state: &mut AppState) {
    let len = state.review_len();
    if len == 0 {
        return;
    }
    state.conflict_cursor = (state.conflict_cursor + 1).min(len - 1);
}

/// The duplicate group under the cursor, when the cursor is past the flags.
pub fn selected_group(state: &AppState) -> Option<&DuplicateGroup> {
    let i = state
        .conflict_cursor
        .checked_sub(state.needs_review.len())?;
    state.duplicates.get(i)
}

/// The `Apply` that deletes every copy of the selected group but the one `keep` names. Deletes go
/// by task id, so line numbers moving inside the batch do not matter; the line goes, no blank
/// is left (as `txtodo conflicts keep-newest`).
pub fn keep_request(state: &AppState, keep: Keep) -> Option<pb::ApplyRequest> {
    let group = selected_group(state)?;
    let kept = match keep {
        Keep::Newest => group.copies.len().checked_sub(1)?,
        Keep::Oldest => 0,
    };
    let mutations: Vec<pb::Mutation> = group
        .copies
        .iter()
        .enumerate()
        .filter(|&(i, _)| i != kept)
        .map(|(_, copy)| pb::Mutation {
            kind: Some(pb::mutation::Kind::Delete(pb::Delete {
                task: Some(pb::TaskRef {
                    line_number: copy.line_number,
                    task_id: copy.task_id.clone(),
                }),
                leave_blank: false,
            })),
        })
        .collect();
    if mutations.is_empty() {
        return None;
    }
    let mut req = crate::commands::apply_of(state, mutations[0].clone());
    req.mutations = mutations;
    Some(req)
}

/// `j`/`k` inside the pane, clamped to the flag list.
pub fn move_up(state: &mut AppState) {
    state.conflict_cursor = state.conflict_cursor.saturating_sub(1);
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
    use crate::state::{ConflictItem, DuplicateCopy};

    fn with_group(state: &mut AppState) {
        let copy = |id: &str, line| DuplicateCopy {
            task_id: id.to_owned(),
            line_number: line,
        };
        state.duplicates.push(DuplicateGroup {
            copies: vec![
                copy("01J9K3H5Z7Q8X2M4N6P8R0T2V1", 2),
                copy("01J9K3H5Z7Q8X2M4N6P8R0T2V2", 5),
                copy("01J9K3H5Z7Q8X2M4N6P8R0T2V3", 7),
            ],
        });
    }

    fn deleted_ids(req: &pb::ApplyRequest) -> Vec<String> {
        req.mutations
            .iter()
            .map(|m| match &m.kind {
                Some(pb::mutation::Kind::Delete(d)) => {
                    assert!(!d.leave_blank);
                    d.task
                        .as_ref()
                        .map(|t| t.task_id.clone())
                        .unwrap_or_default()
                }
                other => panic!("not a delete: {other:?}"),
            })
            .collect()
    }

    #[test]
    fn the_pane_walks_the_flags_then_the_groups() {
        let mut state = two_flags();
        with_group(&mut state);
        assert!(selected_group(&state).is_none(), "on a flag");
        assert!(
            keep_request(&state, Keep::Newest).is_none(),
            "n does nothing on a flag"
        );
        move_down(&mut state);
        move_down(&mut state);
        assert_eq!(state.conflict_cursor, 2);
        assert!(selected_group(&state).is_some());
        assert!(
            resolve_request(&state, Resolution::Mine).is_none(),
            "m does nothing on a group"
        );
        move_down(&mut state);
        assert_eq!(state.conflict_cursor, 2, "clamped at the last group");
    }

    #[test]
    fn keeping_a_copy_deletes_the_others_by_task_id() {
        let mut state = AppState::fixture();
        state.needs_review.clear();
        with_group(&mut state);
        let newest = keep_request(&state, Keep::Newest).unwrap();
        assert_eq!(
            deleted_ids(&newest),
            ["01J9K3H5Z7Q8X2M4N6P8R0T2V1", "01J9K3H5Z7Q8X2M4N6P8R0T2V2"]
        );
        assert_eq!(newest.path, "todo.txt");
        let oldest = keep_request(&state, Keep::Oldest).unwrap();
        assert_eq!(
            deleted_ids(&oldest),
            ["01J9K3H5Z7Q8X2M4N6P8R0T2V2", "01J9K3H5Z7Q8X2M4N6P8R0T2V3"]
        );
    }

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
