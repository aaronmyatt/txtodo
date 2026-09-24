//! The Universal screen's commands (task `tui-revamp/tui-universal`): move over the rows, open or
//! complete one, change the grouping, show done tasks, reset the filters. Pure; opening and
//! completing come back as actions, since they need the daemon.

use crate::action::Action;
use crate::commands::today_local;
use crate::keymap::Command;
use crate::state::AppState;
use crate::state_universal::GROUPS;

/// Runs a Universal command; `None` for any other command.
pub fn run(state: &mut AppState, command: Command) -> Option<Option<Action>> {
    let rows = state.universal.rows(&state.shell.search, &today_local());
    let last = rows.len().saturating_sub(1);
    let cursor = state.universal.cursor;
    let move_to = match command {
        Command::UniversalDown => Some((cursor + 1).min(last)),
        Command::UniversalUp => Some(cursor.saturating_sub(1)),
        Command::UniversalFirst => Some(0),
        Command::UniversalLast => Some(last),
        _ => None,
    };
    if let Some(to) = move_to {
        state.universal.cursor = to;
        return Some(None);
    }
    match command {
        Command::UniversalOpen => return Some(selected(state, &rows).map(Action::OpenUniversal)),
        Command::UniversalComplete => {
            return Some(selected(state, &rows).map(Action::CompleteUniversal));
        }
        _ => {}
    }
    let view = &mut state.universal;
    match command {
        Command::UniversalGroup => {
            let at = GROUPS
                .iter()
                .position(|(g, _)| *g == view.group)
                .unwrap_or(0);
            view.group = GROUPS[(at + 1) % GROUPS.len()].0;
            view.cursor = 0;
        }
        Command::UniversalShowDone => {
            view.show_done = !view.show_done;
            view.cursor = 0;
        }
        Command::UniversalReset => view.reset(),
        _ => return None,
    }
    Some(None)
}

/// The task under the cursor.
fn selected(state: &AppState, rows: &[usize]) -> Option<crate::state_universal::UTask> {
    let i = *rows.get(state.universal.cursor.min(rows.len().saturating_sub(1)))?;
    state.universal.tasks.get(i).cloned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state_universal::UTask;
    use txtodo_core::universal::GroupBy;

    fn state() -> AppState {
        let mut state = AppState::fixture();
        state.universal.tasks = ["(B) two", "(A) one", "three"]
            .iter()
            .enumerate()
            .map(|(i, raw)| UTask {
                workspace_id: "01A".to_owned(),
                workspace: "notes".to_owned(),
                line_number: u32::try_from(i + 1).unwrap_or(0),
                raw: (*raw).to_owned(),
                priority: raw.strip_prefix('(').and_then(|r| r.chars().next()),
                ..UTask::default()
            })
            .collect();
        state
    }

    #[test]
    fn moving_stays_on_the_rows_and_enter_opens_the_one_selected() {
        let mut state = state();
        run(&mut state, Command::UniversalLast);
        assert_eq!(state.universal.cursor, 2);
        run(&mut state, Command::UniversalDown);
        assert_eq!(state.universal.cursor, 2, "stops at the last row");
        run(&mut state, Command::UniversalFirst);
        let Some(Some(Action::OpenUniversal(task))) = run(&mut state, Command::UniversalOpen)
        else {
            panic!("Enter opens");
        };
        assert_eq!(task.raw, "(A) one", "(A) sorts first");
        let Some(Some(Action::CompleteUniversal(task))) =
            run(&mut state, Command::UniversalComplete)
        else {
            panic!("x completes");
        };
        assert_eq!(task.line_number, 2);
    }

    #[test]
    fn the_grouping_cycles_and_reset_clears_the_filters() {
        let mut state = state();
        run(&mut state, Command::UniversalGroup);
        assert_eq!(state.universal.group, GroupBy::Due);
        run(&mut state, Command::UniversalShowDone);
        assert!(state.universal.show_done);
        run(&mut state, Command::UniversalReset);
        assert!(!state.universal.show_done);
    }
}
