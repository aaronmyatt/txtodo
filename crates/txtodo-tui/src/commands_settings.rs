//! The Settings screen's commands (task `tui-revamp/tui-settings`): move between cards and rows,
//! filter the cards, run a row (Enter), remove what a row lists (`d`, twice), and type into a
//! row's field. Pure; what needs the daemon or the disk comes back as [`SettingsAction`].

use crate::action::Action;
use crate::keymap::Command;
use crate::settings_rows::{Act, SRow, card_matches, rows};
use crate::state::AppState;
use crate::state_nav::{Screen, SettingsCard};
use crate::state_settings::Pairing;
use crate::state_shell::Link;
use crate::theme::ThemeMode;

/// What Settings asks of the daemon or the disk.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SettingsAction {
    /// Read the workspaces, devices, tokens and activity again.
    Refresh,
    /// Register the folder at this path.
    AddWorkspace(String),
    /// Unregister this workspace.
    RemoveWorkspace(String),
    /// Accept this pairing code.
    PairAccept(String),
    /// The six words matched; `true` when it is the user's own device.
    PairConfirm(bool),
    /// Revoke this device.
    RevokeDevice(String),
    /// Create a token from `name scope… expires:YYYY-MM-DD`.
    CreateToken(String),
    /// Revoke this token.
    RevokeToken(String),
    /// Write the preferences file.
    SavePrefs,
}

/// The card in view, if Settings is showing.
fn card(state: &AppState) -> Option<SettingsCard> {
    match state.nav.screen {
        Screen::Settings(card) => Some(card),
        _ => None,
    }
}

/// The cards the filter keeps, in nav order.
pub fn visible_cards(state: &AppState) -> Vec<SettingsCard> {
    SettingsCard::ALL
        .into_iter()
        .filter(|c| card_matches(*c, state, &state.settings.filter))
        .collect()
}

/// Runs a Settings command; `None` for any other command.
pub fn run(state: &mut AppState, command: Command) -> Option<Option<Action>> {
    let card = card(state)?;
    let rows = rows(card, state);
    let settings = &mut state.settings;
    Some(match command {
        Command::SettingsDown => {
            settings.row = (settings.row + 1).min(rows.len().saturating_sub(1));
            settings.confirm = None;
            None
        }
        Command::SettingsUp => {
            settings.row = settings.row.saturating_sub(1);
            settings.confirm = None;
            None
        }
        Command::SettingsNextCard => step_card(state, card, true),
        Command::SettingsPrevCard => step_card(state, card, false),
        Command::SettingsFilter => {
            settings.filtering = true;
            None
        }
        Command::SettingsActivate => activate(state, rows.get(state.settings.row)?.clone()),
        Command::SettingsRemove => remove(state, rows.get(state.settings.row)?.clone()),
        _ => return None,
    })
}

/// Shows the next (or previous) card the filter keeps, at its first row.
fn step_card(state: &mut AppState, card: SettingsCard, forward: bool) -> Option<Action> {
    let cards = visible_cards(state);
    let at = cards.iter().position(|c| *c == card).unwrap_or(0);
    let next = if forward {
        (at + 1) % cards.len().max(1)
    } else {
        (at + cards.len().max(1) - 1) % cards.len().max(1)
    };
    show_card(state, *cards.get(next)?)
}

/// Shows `card` at its first row; the Activity card reads the feed on the way in.
pub fn show_card(state: &mut AppState, card: SettingsCard) -> Option<Action> {
    state.nav.screen = Screen::Settings(card);
    state.settings.row = 0;
    state.settings.field = None;
    state.settings.confirm = None;
    (card == SettingsCard::Activity).then_some(Action::Settings(SettingsAction::Refresh))
}

/// Enter on a row: starts or submits its field, or runs its act.
fn activate(state: &mut AppState, row: SRow) -> Option<Action> {
    let act = row.act?;
    if row.field {
        let Some(text) = state.settings.field.take() else {
            state.settings.field = Some(String::new());
            return None;
        };
        let text = text.trim().to_owned();
        if text.is_empty() {
            return None;
        }
        return Some(Action::Settings(match act {
            Act::AddWorkspace => SettingsAction::AddWorkspace(text),
            Act::PairCode => SettingsAction::PairAccept(text),
            _ => SettingsAction::CreateToken(text),
        }));
    }
    run_act(state, act)
}

/// What a row's act does.
fn run_act(state: &mut AppState, act: Act) -> Option<Action> {
    let prefs = &mut state.settings.prefs;
    let save = Some(Action::Settings(SettingsAction::SavePrefs));
    match act {
        Act::Theme => {
            prefs.theme = match prefs.theme {
                ThemeMode::System => ThemeMode::Light,
                ThemeMode::Light => ThemeMode::Dark,
                ThemeMode::Dark => ThemeMode::System,
            };
            save
        }
        Act::LineNumbers => {
            prefs.line_numbers = !prefs.line_numbers;
            save
        }
        Act::LengthHint => {
            prefs.length_hint = !prefs.length_hint;
            save
        }
        Act::Retry => {
            if state.shell.link != Link::Up {
                state.shell.link = Link::Connecting;
            }
            None
        }
        Act::OpenWorkspace(id) => Some(Action::SwitchWorkspace(id)),
        Act::SasMatch(own) => Some(Action::Settings(SettingsAction::PairConfirm(own))),
        Act::SasDiffer => {
            state.settings.pairing = Pairing::Idle;
            let message = "Codes differ. Pairing stopped; nothing was shared.";
            state.shell.toast(message, None, std::time::Instant::now());
            None
        }
        Act::AcceptOffer(i) => {
            state.offers.cursor = i;
            crate::ui::offers::accept_request(state).map(Action::AcceptOffer)
        }
        Act::CopySecret => {
            let secret = state.settings.secret.take()?;
            state
                .shell
                .toast("Secret copied", None, std::time::Instant::now());
            Some(Action::Copy(secret))
        }
        Act::RefreshActivity => Some(Action::Settings(SettingsAction::Refresh)),
        _ => None,
    }
}

/// `d` on a row: the first press asks, the second removes.
fn remove(state: &mut AppState, row: SRow) -> Option<Action> {
    let act = row.remove?;
    if state.settings.confirm != Some(state.settings.row) {
        state.settings.confirm = Some(state.settings.row);
        return None;
    }
    state.settings.confirm = None;
    Some(match act {
        Act::RemoveWorkspace(id) => Action::Settings(SettingsAction::RemoveWorkspace(id)),
        Act::RevokeDevice(id) => Action::Settings(SettingsAction::RevokeDevice(id)),
        Act::RevokeToken(id) => Action::Settings(SettingsAction::RevokeToken(id)),
        Act::DeclineOffer(i) => {
            state.offers.cursor = i;
            return crate::ui::offers::decline_request(state).map(Action::DeclineOffer);
        }
        _ => return None,
    })
}

/// A key while a Settings field or the card filter has the keyboard: Enter submits (the filter
/// just stays), Esc cancels, Backspace deletes, a character types.
pub fn on_text_key(state: &mut AppState, key: crossterm::event::KeyEvent) -> Option<Action> {
    use crossterm::event::KeyCode;
    let settings = &mut state.settings;
    let filtering = settings.filtering;
    match key.code {
        KeyCode::Enter if filtering => settings.filtering = false,
        KeyCode::Enter => return run(state, Command::SettingsActivate).flatten(),
        KeyCode::Esc if filtering => {
            settings.filtering = false;
            settings.filter.clear();
        }
        KeyCode::Esc => settings.field = None,
        KeyCode::Backspace => {
            if filtering {
                settings.filter.pop();
            } else if let Some(f) = settings.field.as_mut() {
                f.pop();
            }
        }
        KeyCode::Char(c) => {
            if filtering {
                settings.filter.push(c);
            } else if let Some(f) = settings.field.as_mut() {
                f.push(c);
            }
        }
        _ => {}
    }
    if filtering {
        let cards = visible_cards(state);
        if let (Some(card), Some(first)) = (card(state), cards.first())
            && !cards.contains(&card)
        {
            show_card(state, *first);
        }
    }
    None
}

#[cfg(test)]
#[path = "commands_settings_tests.rs"]
mod tests;
