//! The shell's commands (task `tui-revamp/tui-shell`): `g t` / `g u` / `g s` / `?` move between
//! screens, `W` opens the workspace popup, whose rows switch workspace or go to Settings ›
//! Workspaces, and the banners' buttons. Split from `commands.rs` for its line budget; pure like
//! it.

use crate::action::Action;
use crate::keymap::Command;
use crate::state::AppState;
use crate::state_nav::{Overlay, Screen, SettingsCard};
use crate::state_shell::Link;

/// Runs a screen or popup command; `None` for any other command.
pub fn run(state: &mut AppState, command: Command) -> Option<Option<Action>> {
    let screen = match command {
        Command::NavTasks => Screen::Tasks,
        Command::NavUniversal => Screen::Universal,
        Command::NavSettings => Screen::Settings(SettingsCard::General),
        Command::NavHelp => Screen::Help,
        Command::NavWorkspaceMenu | Command::WorkspaceSwitch => {
            return Some(Some(Action::OpenWorkspaceMenu));
        }
        other => return run_menu(state, other).or_else(|| run_banner(state, other)),
    };
    go(state, screen);
    // Universal and Settings read what no Watch covers: read it on the way in.
    Some(match state.nav.screen {
        Screen::Universal => Some(Action::RefreshUniversal),
        Screen::Settings(_) => Some(Action::Settings(
            crate::commands_settings::SettingsAction::Refresh,
        )),
        _ => None,
    })
}

/// Shows `screen`, closing any popup; a Settings screen already open keeps its card.
pub fn go(state: &mut AppState, screen: Screen) {
    let same_settings = matches!(
        (state.nav.screen, screen),
        (Screen::Settings(_), Screen::Settings(SettingsCard::General))
    );
    if !same_settings {
        state.nav.screen = screen;
    }
    state.nav.overlay = None;
}

fn run_menu(state: &mut AppState, command: Command) -> Option<Option<Action>> {
    let menu = &mut state.shell.menu;
    match command {
        Command::WorkspaceMenuDown => menu.move_down(),
        Command::WorkspaceMenuUp => menu.move_up(),
        Command::WorkspaceMenuClose => state.nav.overlay = None,
        Command::WorkspaceMenuOpen => {
            state.nav.overlay = None;
            let Some(item) = state.shell.menu.selected() else {
                go(state, Screen::Settings(SettingsCard::Workspaces));
                return Some(None);
            };
            if item.current {
                return Some(None);
            }
            return Some(Some(Action::SwitchWorkspace(item.id.clone())));
        }
        _ => return None,
    }
    Some(None)
}

/// The banners' buttons.
fn run_banner(state: &mut AppState, command: Command) -> Option<Option<Action>> {
    match command {
        Command::AppRetryDaemon if state.shell.link != Link::Up => {
            state.shell.link = Link::Connecting;
        }
        Command::AppDismissSkillHint => state.skill_hint = false,
        Command::ConflictsDismissBanner => state.shell.conflict_banner_hidden = true,
        Command::AppCopyRefusedEdit => {
            let refused = state.shell.refused.take()?;
            return Some(Some(Action::Copy(refused.text)));
        }
        Command::AppRetryDaemon => {}
        Command::HelpDown => state.shell.help_scroll = state.shell.help_scroll.saturating_add(1),
        Command::HelpUp => state.shell.help_scroll = state.shell.help_scroll.saturating_sub(1),
        Command::ToastUndo => {
            state.shell.undoable()?;
            state.shell.toasts.pop();
            return Some(undo_last(state));
        }
        Command::ListUndo => return Some(undo_last(state)),
        _ => return None,
    }
    Some(None)
}

/// `u` and a toast's Undo: takes back this session's newest change, all its ops, through the
/// daemon. Changes from before the session, or from other clients, are not the TUI's to guess at.
fn undo_last(state: &mut AppState) -> Option<Action> {
    let Some(change) = state.shell.changes.pop() else {
        state.last_error = Some("nothing to undo from this session".to_owned());
        return None;
    };
    Some(Action::Undo(change.path, change.ops, change.workspace))
}

/// Opens the popup over `items` (after `app` fetched them).
pub fn open_menu(state: &mut AppState, items: Vec<crate::state_shell::MenuItem>) {
    state.shell.menu.fill(items);
    state.nav.overlay = Some(Overlay::WorkspaceMenu);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state_shell::MenuItem;

    #[test]
    fn screen_commands_move_between_screens_and_close_the_popup() {
        let mut state = AppState::fixture();
        state.nav.overlay = Some(Overlay::WorkspaceMenu);
        assert_eq!(
            run(&mut state, Command::NavUniversal),
            Some(Some(Action::RefreshUniversal)),
            "Universal is read on the way in"
        );
        assert_eq!(state.nav.screen, Screen::Universal);
        assert_eq!(state.nav.overlay, None);
        state.nav.screen = Screen::Settings(SettingsCard::Tokens);
        run(&mut state, Command::NavSettings);
        assert_eq!(
            state.nav.screen,
            Screen::Settings(SettingsCard::Tokens),
            "keeps its card"
        );
        assert_eq!(
            run(&mut state, Command::NavWorkspaceMenu),
            Some(Some(Action::OpenWorkspaceMenu))
        );
        assert_eq!(run(&mut state, Command::ListDown), None, "not ours");
    }

    #[test]
    fn the_popup_switches_to_another_workspace_or_manages_them() {
        let item = |id: &str, current| MenuItem {
            id: id.to_owned(),
            current,
            ..MenuItem::default()
        };
        let mut state = AppState::fixture();
        open_menu(&mut state, vec![item("01A", true), item("01B", false)]);
        assert_eq!(state.nav.overlay, Some(Overlay::WorkspaceMenu));
        assert_eq!(
            run(&mut state, Command::WorkspaceMenuOpen),
            Some(None),
            "already open"
        );
        open_menu(&mut state, vec![item("01A", true), item("01B", false)]);
        run(&mut state, Command::WorkspaceMenuDown);
        assert_eq!(
            run(&mut state, Command::WorkspaceMenuOpen),
            Some(Some(Action::SwitchWorkspace("01B".to_owned())))
        );
        open_menu(&mut state, vec![item("01A", true)]);
        run(&mut state, Command::WorkspaceMenuDown);
        run(&mut state, Command::WorkspaceMenuOpen);
        assert_eq!(state.nav.screen, Screen::Settings(SettingsCard::Workspaces));
    }

    #[test]
    fn u_takes_back_this_sessions_newest_change_and_a_stale_toast_does_not() {
        let now = std::time::Instant::now();
        let mut state = AppState::fixture();
        assert_eq!(run(&mut state, Command::ListUndo), Some(None));
        assert_eq!(
            state.last_error.as_deref(),
            Some("nothing to undo from this session")
        );
        let deleted = state.shell.record("todo.txt", 2);
        state.shell.toast("Deleted the line", Some(deleted), now);
        state.shell.record("todo.txt", 1); // a move, with no toast
        assert_eq!(
            run(&mut state, Command::ToastUndo),
            None,
            "the toast is stale"
        );
        assert_eq!(
            run(&mut state, Command::ListUndo),
            Some(Some(Action::Undo("todo.txt".to_owned(), 1, None))),
            "u takes back the move"
        );
        assert_eq!(
            run(&mut state, Command::ToastUndo),
            Some(Some(Action::Undo("todo.txt".to_owned(), 2, None))),
            "now the toast's change is the newest again"
        );
    }
}
