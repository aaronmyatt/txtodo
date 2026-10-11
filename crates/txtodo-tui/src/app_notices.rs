//! Change notices in the TUI (task `change-notifications`): another client's edits, completions
//! and deletions (an agent, the CLI, another device, an editor save) come back as one toast per
//! burst. The folding and the words are `txtodo_core::notice`'s, shared with the desktop; this file
//! only feeds it the `Watch` ops and shows what it says. The TUI's own ops (source `tui`) never
//! count.

use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use txtodo_core::notice::OpFacts;
use txtodo_proto::v1 as pb;

use crate::state::AppState;

/// Wall-clock ms; the debouncer only compares them with each other.
/// Ref: <https://doc.rust-lang.org/std/time/struct.SystemTime.html#method.duration_since>
fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

/// Feeds one `Watch` change's ops to the debouncer.
pub fn take(state: &mut AppState, change: &pb::Change) {
    take_at(state, change, now_ms());
}

fn take_at(state: &mut AppState, change: &pb::Change, now: u64) {
    for op in &change.ops {
        state.shell.notices.push(
            now,
            OpFacts {
                path: &change.path,
                source: &op.source,
                kind: &op.kind,
                task_id: &op.task_id,
                summary: &op.summary,
            },
        );
    }
}

/// When the pending notice is due, for the loop's deadline.
pub fn due(state: &AppState) -> Option<Instant> {
    let at = state.shell.notices.next_due()?;
    Some(Instant::now() + Duration::from_millis(at.saturating_sub(now_ms())))
}

/// Shows the notice as a toast once it is due.
pub fn show_due(state: &mut AppState) {
    show_due_at(state, now_ms(), Instant::now());
}

fn show_due_at(state: &mut AppState, now: u64, at: Instant) {
    if let Some(notice) = state.shell.notices.due(now) {
        let message = format!("{} \u{b7} {}", notice.title, notice.body);
        state.shell.toast(message, None, at);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use txtodo_core::notice::QUIET_MS;

    fn change(source: &str) -> pb::Change {
        pb::Change {
            path: "todo.txt".into(),
            ops: vec![pb::OpSummary {
                source: source.into(),
                kind: "set_field".into(),
                task_id: "T1".into(),
                summary: "Completed = Bool(true)".into(),
                ..pb::OpSummary::default()
            }],
            ..pb::Change::default()
        }
    }

    #[test]
    fn an_agents_completion_toasts_once_quiet() {
        let mut state = AppState::fixture();
        let at = Instant::now();
        take_at(&mut state, &change("mcp"), 1_000);
        show_due_at(&mut state, 1_000, at);
        assert!(state.shell.toasts.is_empty(), "not quiet long enough yet");
        show_due_at(&mut state, 1_000 + QUIET_MS, at);
        let toasts: Vec<_> = state
            .shell
            .toasts
            .iter()
            .map(|t| t.message.as_str())
            .collect();
        assert_eq!(
            toasts,
            ["A task was completed by an agent \u{b7} in todo.txt"]
        );
    }

    #[test]
    fn the_tuis_own_ops_stay_quiet() {
        let mut state = AppState::fixture();
        take_at(&mut state, &change("tui"), 0);
        assert_eq!(due(&state), None);
    }
}
