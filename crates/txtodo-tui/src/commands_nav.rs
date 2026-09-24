//! The screen and workspace-popup commands (task `tui-revamp/tui-shell`): `g t` / `g u` / `g s` /
//! `?` move between screens, `W` opens the workspace popup, and its rows switch workspace or go to
//! Settings › Workspaces. Split from `commands.rs` for its line budget; pure like it.

use crate::action::Action;
use crate::keymap::Command;
use crate::state::AppState;
use crate::state_nav::{Overlay, Screen, SettingsCard};

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
        other => return run_menu(state, other),
    };
    go(state, screen);
    Some(None)
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
        assert_eq!(run(&mut state, Command::NavUniversal), Some(None));
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
}
