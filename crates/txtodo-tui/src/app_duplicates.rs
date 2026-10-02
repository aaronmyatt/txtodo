//! The open file's duplicate groups (ADR 0032, task sync-drift duplicate-flags), read with
//! `ListConflicts`. The daemon derives them from the file when asked; `Change.duplicate_groups`
//! only counts them, so the TUI re-reads the groups at startup and after a change to the open
//! document that has or had any.

use txtodo_proto::v1 as pb;

use crate::daemon::Daemon;
use crate::state::{AppState, DuplicateCopy, DuplicateGroup};

/// Re-reads the groups; a failed read keeps the old ones. The review cursor stays in range.
pub async fn refresh(daemon: &mut Daemon, state: &mut AppState) {
    if let Ok(found) = daemon.list_conflicts(&state.path).await {
        set(state, found.duplicates);
    }
}

/// After a `Watch` change to the open document: worth a read when the file has groups now
/// (line numbers move) or had some before (they may be gone).
pub fn worth_refresh(state: &AppState, change: &pb::Change) -> bool {
    change.duplicate_groups > 0 || !state.duplicates.is_empty()
}

/// Replaces the groups with the daemon's.
pub fn set(state: &mut AppState, groups: Vec<pb::DuplicateGroup>) {
    state.duplicates = groups
        .into_iter()
        .map(|g| DuplicateGroup {
            copies: g
                .tasks
                .into_iter()
                .map(|t| DuplicateCopy {
                    task_id: t.task_id,
                    line_number: t.line_number,
                })
                .collect(),
        })
        .filter(|g| g.copies.len() > 1)
        .collect();
    state.conflict_cursor = state
        .conflict_cursor
        .min(state.review_len().saturating_sub(1));
    if state.review_len() == 0 {
        state.conflicts_open = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn group(lines: &[u32]) -> pb::DuplicateGroup {
        pb::DuplicateGroup {
            tasks: lines
                .iter()
                .map(|&n| pb::DuplicateTask {
                    task_id: format!("01J9K3H5Z7Q8X2M4N6P8R0T2V{n}"),
                    line_number: n,
                })
                .collect(),
        }
    }

    #[test]
    fn the_groups_replace_the_old_ones_and_the_sheet_closes_when_none_are_left() {
        let mut state = AppState::fixture();
        state.needs_review.clear();
        set(&mut state, vec![group(&[1, 3]), group(&[2, 4])]);
        assert_eq!(state.duplicates.len(), 2);
        state.conflicts_open = true;
        state.conflict_cursor = 1;
        set(&mut state, vec![group(&[1, 3])]);
        assert_eq!(state.conflict_cursor, 0, "clamped");
        assert!(state.conflicts_open);
        set(&mut state, Vec::new());
        assert!(!state.conflicts_open, "nothing left to review");
    }

    #[test]
    fn a_duplicate_line_stays_editable() {
        let mut state = AppState::from_document("todo.txt", "buy milk\nbuy milk\n");
        set(&mut state, vec![group(&[1, 2])]);
        assert!(
            !crate::commands::under_review(&state, 0),
            "ADR 0032: never read-only"
        );
        assert!(!crate::commands::under_review(&state, 1));
    }

    #[test]
    fn a_change_is_worth_a_read_when_the_file_has_or_had_groups() {
        let mut state = AppState::fixture();
        let quiet = pb::Change::default();
        assert!(!worth_refresh(&state, &quiet));
        let counted = pb::Change {
            duplicate_groups: 1,
            ..pb::Change::default()
        };
        assert!(worth_refresh(&state, &counted));
        set(&mut state, vec![group(&[1, 2])]);
        assert!(worth_refresh(&state, &quiet), "the last group may be gone");
    }
}
