//! The `r` conflicts sheet: walks the file's duplicate groups (ADR 0032), two or more lines that
//! read the same. `n`/`o` keep the newest or the oldest copy and delete the rest with an ordinary
//! `Apply` `Delete` by task id, as `txtodo conflicts` does; there is no resolve RPC for them.
//! Same-word review (`needs_review` flags, Keep mine/theirs) went with the todo.txt mirror
//! (ADR 0038).

use txtodo_proto::v1 as pb;

use crate::state::{AppState, DuplicateGroup};

/// Which copy of a duplicate group survives `keep_request`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Keep {
    /// The newest task id (the last copy): the safe default (ADR 0032).
    Newest,
    /// The oldest task id (the first copy).
    Oldest,
}

/// `j` inside the sheet, clamped to the groups.
pub fn move_down(state: &mut AppState) {
    let len = state.review_len();
    if len == 0 {
        return;
    }
    state.conflict_cursor = (state.conflict_cursor + 1).min(len - 1);
}

/// The duplicate group under the cursor.
pub fn selected_group(state: &AppState) -> Option<&DuplicateGroup> {
    state.duplicates.get(state.conflict_cursor)
}

/// The `Apply` that deletes every copy of the selected group but the one `keep` names. Deletes go
/// by task id alone (`line_number: 0`): a line number would be resolved first, and the second
/// delete's line has moved by then (under Sidecar the daemon would delete whatever is there now;
/// `txtodo-daemon/src/mutation.rs::resolve`). The line goes, no blank is left (as
/// `txtodo conflicts keep-newest`).
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
                    line_number: 0,
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

/// `k` inside the sheet.
pub fn move_up(state: &mut AppState) {
    state.conflict_cursor = state.conflict_cursor.saturating_sub(1);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::DuplicateCopy;

    fn with_group(state: &mut AppState, first: u32) {
        let copy = |n: u32| DuplicateCopy {
            task_id: format!("01J9K3H5Z7Q8X2M4N6P8R0T2V{n}"),
            line_number: n,
        };
        state.duplicates.push(DuplicateGroup {
            copies: vec![copy(first), copy(first + 1), copy(first + 2)],
        });
    }

    fn deleted_ids(req: &pb::ApplyRequest) -> Vec<String> {
        req.mutations
            .iter()
            .map(|m| match &m.kind {
                Some(pb::mutation::Kind::Delete(d)) => {
                    assert!(!d.leave_blank);
                    assert_eq!(
                        d.task.as_ref().map(|t| t.line_number),
                        Some(0),
                        "by id alone"
                    );
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
    fn the_sheet_walks_the_groups_and_clamps() {
        let mut state = AppState::fixture();
        assert!(selected_group(&state).is_none(), "no groups");
        assert!(keep_request(&state, Keep::Newest).is_none());
        with_group(&mut state, 1);
        with_group(&mut state, 5);
        move_up(&mut state);
        assert_eq!(state.conflict_cursor, 0);
        move_down(&mut state);
        move_down(&mut state);
        assert_eq!(state.conflict_cursor, 1, "clamped at the last group");
        assert_eq!(
            selected_group(&state).map(|g| g.copies[0].line_number),
            Some(5)
        );
    }

    #[test]
    fn keeping_a_copy_deletes_the_others_by_task_id() {
        let mut state = AppState::fixture();
        with_group(&mut state, 1);
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
}
