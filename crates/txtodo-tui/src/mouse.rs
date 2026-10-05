//! Mouse events (task `tui-revamp/tui-mouse`), looked up in the last frame's [`crate::hit::HitMap`]: a click
//! selects a row or runs a clickable's [`Command`], a double-click edits the row, the wheel
//! scrolls the list, the pointer hovers a row, and dragging a row drops it somewhere else. Pure and
//! terminal-free, so it is tested over a fixed map.
//!
//! crossterm reports no double-click: two left presses on the same cell within [`DOUBLE_CLICK`]
//! make one. While a line is being edited, a press on it moves the caret and a press anywhere
//! else saves it (`mouse_text`); a release on the already-selected row's text starts editing it
//! there. While the `:` line is open or a sheet is up, the keyboard has the screen.
//! Ref: <https://docs.rs/crossterm/latest/crossterm/event/struct.MouseEvent.html>

use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};

use crate::action::Action;
use crate::commands;
use crate::hit::Target;
use crate::keymap::Command;
use crate::state::AppState;

/// How soon a second press on the same cell counts as a double-click (the tui-mouse plan).
pub const DOUBLE_CLICK: Duration = Duration::from_millis(400);

/// Rows one wheel notch scrolls.
const WHEEL_ROWS: isize = 3;

/// What the mouse remembers between events: the last press, for double-clicks, and the row being
/// dragged.
#[derive(Clone, Debug, Default)]
pub struct Mouse {
    last_press: Option<(u16, u16, Instant)>,
    dragging: Option<usize>,
    /// A press on the already-selected row's text: its release (no drag) starts an edit there.
    armed: Option<Arm>,
    /// The text an edit started by a click began with, so a quick second press on an untouched
    /// draft can be read as the double-click that opens the row.
    click_edit: Option<String>,
}

/// A pending click-to-edit: the row, whether it is the sub-list's, and where its rows start.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Arm {
    row: usize,
    sub: bool,
    left: u16,
}

impl Mouse {
    /// Handles one event at `now`, returning the [`Action`] to perform, if any.
    pub fn on_event(
        &mut self,
        state: &mut AppState,
        ev: MouseEvent,
        now: Instant,
    ) -> Option<Action> {
        let target = state.hits.at(ev.column, ev.row);
        if state.editing.is_some() {
            self.dragging = None;
            self.armed = None;
            return match ev.kind {
                MouseEventKind::Down(MouseButton::Left) => {
                    self.press_editing(state, target, &ev, now)
                }
                _ => None,
            };
        }
        self.click_edit = None;
        if busy(state) {
            state.hover = None;
            self.dragging = None;
            return None;
        }
        match ev.kind {
            MouseEventKind::Moved | MouseEventKind::Drag(MouseButton::Left) => {
                state.hover = row_of(target);
                None
            }
            MouseEventKind::ScrollDown => wheel(state, target, true),
            MouseEventKind::ScrollUp => wheel(state, target, false),
            MouseEventKind::Down(MouseButton::Left) => self.press(state, target, &ev, now),
            MouseEventKind::Up(MouseButton::Left) => self.release(state, target, ev.column),
            _ => None,
        }
    }

    fn press(
        &mut self,
        state: &mut AppState,
        target: Option<Target>,
        ev: &MouseEvent,
        now: Instant,
    ) -> Option<Action> {
        let row = match target? {
            Target::Row(row) => row,
            Target::Command(command) => return commands::run(state, command),
            Target::MenuItem(index) => {
                state.shell.menu.cursor = index;
                return commands::run(state, Command::WorkspaceMenuOpen);
            }
            Target::Suggestion(index) => {
                crate::search::pick_suggestion(state, index);
                return None;
            }
            Target::Crumb(keep) => return crate::commands_detail::leave(state, keep),
            Target::Chip(index) => {
                crate::prompt::apply(state, index);
                return None;
            }
            Target::UniversalRow(_)
            | Target::UniversalGroup(_)
            | Target::UniversalWorkspace(_)
            | Target::UniversalContext(_) => {
                let t = target?;
                return self.press_universal(state, t, ev, now);
            }
            Target::SettingsCard(i) => {
                let card = crate::state_nav::SettingsCard::ALL.get(i).copied()?;
                return crate::commands_settings::show_card(state, card);
            }
            Target::SettingsRow(i) => {
                let double = self.is_double(ev, now);
                state.settings.row = i;
                state.settings.confirm = None;
                return double
                    .then(|| commands::run(state, Command::SettingsActivate))
                    .flatten();
            }
            Target::DetailRow(index) => return self.press_sub_row(state, index, ev, now),
            Target::Field(which, area) => {
                crate::mouse_text::press_field(state, which, area, ev.column, ev.row);
                return None;
            }
            Target::Inert => return None,
        };
        let double = self.is_double(ev, now);
        let was_selected = state.cursor == row && state.nav.focus == crate::state_nav::Focus::List;
        if was_selected && !double {
            let left = state.hits.list().map_or(0, |l| l.area.x);
            self.armed = Some(Arm {
                row,
                sub: false,
                left,
            });
        }
        state.cursor = row;
        // A click on the list takes the keyboard back from the search field or the panel.
        state.nav.focus = crate::state_nav::Focus::List;
        if double {
            self.dragging = None;
            return commands::run(state, Command::DetailOpen);
        }
        self.dragging = Some(row);
        None
    }

    /// A click on a row of the detail panel's sub-list: selects it and gives the sub-list the
    /// keyboard; a double-click opens it a level deeper.
    fn press_sub_row(
        &mut self,
        state: &mut AppState,
        index: usize,
        ev: &MouseEvent,
        now: Instant,
    ) -> Option<Action> {
        let double = self.is_double(ev, now);
        self.dragging = None;
        let selected = state.detail.top().map(|l| l.doc.cursor);
        let sub_has_keys = crate::mouse_text::in_detail(state)
            && state.detail.part == crate::state_detail::Part::Sub;
        if sub_has_keys && selected == Some(index) && !double {
            let left = state
                .hits
                .area_at(ev.column, ev.row)
                .map_or(0, |(r, _)| r.x);
            self.armed = Some(Arm {
                row: index,
                sub: true,
                left,
            });
        }
        state.nav.focus = crate::state_nav::Focus::Detail;
        state.detail.part = crate::state_detail::Part::Sub;
        crate::state_detail::with_sub_list(state, |s| {
            s.cursor = index;
            if double {
                commands::run(s, Command::DetailOpen)
            } else {
                None
            }
        })
        .flatten()
    }

    /// A click on the Universal screen: a row selects (a double-click opens it), a grouping,
    /// workspace or context chip switches its filter.
    fn press_universal(
        &mut self,
        state: &mut AppState,
        target: Target,
        ev: &MouseEvent,
        now: Instant,
    ) -> Option<Action> {
        let view = &mut state.universal;
        match target {
            Target::UniversalRow(row) => {
                let double = self.is_double(ev, now);
                view.cursor = row;
                if double {
                    return commands::run(state, Command::UniversalOpen);
                }
            }
            Target::UniversalGroup(i) => {
                if let Some((group, _)) = crate::state_universal::GROUPS.get(i) {
                    view.group = *group;
                    view.cursor = 0;
                }
            }
            Target::UniversalWorkspace(i) => {
                if let Some((id, _, _)) = view.workspaces().get(i) {
                    view.toggle_workspace(&id.clone());
                }
            }
            Target::UniversalContext(i) => {
                let query = state.shell.search.clone();
                let name = state
                    .universal
                    .contexts(&query)
                    .get(i)
                    .map(|(c, _)| c.clone());
                if let Some(name) = name {
                    state.universal.toggle_context(&name);
                }
            }
            _ => {}
        }
        None
    }

    /// Whether this press, at `now`, is the second of a double-click; remembers it otherwise.
    fn is_double(&mut self, ev: &MouseEvent, now: Instant) -> bool {
        let double = self.last_press.is_some_and(|(column, line, at)| {
            (column, line) == (ev.column, ev.row) && now.duration_since(at) <= DOUBLE_CLICK
        });
        self.last_press = if double {
            None
        } else {
            Some((ev.column, ev.row, now))
        };
        double
    }

    fn release(
        &mut self,
        state: &mut AppState,
        target: Option<Target>,
        column: u16,
    ) -> Option<Action> {
        let armed = self.armed.take();
        let from = self.dragging.take();
        match (target?, from) {
            (Target::Row(to), Some(from)) if to != from => commands::move_row(state, from, to),
            (Target::Row(to) | Target::DetailRow(to), _) if armed.is_some_and(|a| a.row == to) => {
                let arm = armed?;
                self.click_edit = crate::mouse_text::edit_at(state, arm.sub, arm.left, column);
                None
            }
            _ => None,
        }
    }

    /// A press while a line is being edited: on the line, the caret moves there (or, the quick
    /// second press of a double-click on a draft a click just opened and nobody typed in, the row
    /// opens instead); anywhere else, the edit is saved.
    fn press_editing(
        &mut self,
        state: &mut AppState,
        target: Option<Target>,
        ev: &MouseEvent,
        now: Instant,
    ) -> Option<Action> {
        let double = self.is_double(ev, now);
        if let Some(Target::Field(crate::hit::Field::LineEdit, area)) = target {
            let untouched = self
                .click_edit
                .as_ref()
                .is_some_and(|text| state.editing.as_ref().is_some_and(|d| &d.buffer == text));
            if double && untouched {
                self.click_edit = None;
                return crate::mouse_text::open_instead(state);
            }
            crate::mouse_text::press_field(
                state,
                crate::hit::Field::LineEdit,
                area,
                ev.column,
                ev.row,
            );
            return None;
        }
        self.click_edit = None;
        crate::mouse_text::save_edit(state)
    }
}

/// Whether something other than the list has the keyboard (the line editor is handled first).
fn busy(state: &AppState) -> bool {
    state.command.is_some() || state.conflicts_open || state.offers.open
}

fn row_of(target: Option<Target>) -> Option<usize> {
    match target? {
        Target::Row(row) => Some(row),
        Target::Command(_)
        | Target::MenuItem(_)
        | Target::Suggestion(_)
        | Target::Crumb(_)
        | Target::DetailRow(_)
        | Target::Chip(_)
        | Target::UniversalRow(_)
        | Target::UniversalGroup(_)
        | Target::UniversalWorkspace(_)
        | Target::UniversalContext(_)
        | Target::SettingsCard(_)
        | Target::SettingsRow(_)
        | Target::Field(..)
        | Target::Inert => None,
    }
}

/// The wheel over the detail panel's sub-list moves its cursor, over its notes moves their caret,
/// on the Universal and Settings screens moves their selection, anywhere else scrolls the list.
fn wheel(state: &mut AppState, target: Option<Target>, down: bool) -> Option<Action> {
    let steps = WHEEL_ROWS.unsigned_abs();
    match target {
        Some(Target::DetailRow(_)) => {
            let step = if down {
                Command::ListDown
            } else {
                Command::ListUp
            };
            crate::state_detail::with_sub_list(state, |s| repeat(s, step, steps));
            None
        }
        Some(Target::Field(crate::hit::Field::Notes, _)) => {
            let notes = &mut state.detail.top_mut()?.notes;
            let code = if down { KeyCode::Down } else { KeyCode::Up };
            for _ in 0..steps {
                let key = KeyEvent::new(code, KeyModifiers::NONE);
                crate::ui::notes_edit::on_key(notes, key, Instant::now());
            }
            None
        }
        _ => screen_wheel(state, down, steps),
    }
}

/// The wheel anywhere else: a screen's own selection, or the list's view.
fn screen_wheel(state: &mut AppState, down: bool, steps: usize) -> Option<Action> {
    let (next, prev) = match state.nav.screen {
        crate::state_nav::Screen::Settings(_) => (Command::SettingsDown, Command::SettingsUp),
        crate::state_nav::Screen::Universal => (Command::UniversalDown, Command::UniversalUp),
        crate::state_nav::Screen::Help => (Command::HelpDown, Command::HelpUp),
        _ => return scroll(state, if down { WHEEL_ROWS } else { -WHEEL_ROWS }),
    };
    repeat(state, if down { next } else { prev }, steps);
    None
}

/// Runs a movement command `steps` times.
fn repeat(state: &mut AppState, command: Command, steps: usize) {
    for _ in 0..steps {
        commands::run(state, command);
    }
}

/// Moves the view by `rows`, clamped to the list, and pulls the cursor into it: drawing would
/// otherwise scroll straight back to the cursor.
fn scroll(state: &mut AppState, rows: isize) -> Option<Action> {
    let height = usize::from(state.hits.list()?.area.height).max(1);
    let last_row = state.row_count() - 1;
    let top = state.scroll.saturating_add_signed(rows);
    state.scroll = top.min(state.row_count().saturating_sub(height));
    let bottom = (state.scroll + height - 1).min(last_row);
    let row = state.row_of(state.cursor).clamp(state.scroll, bottom);
    state.cursor = state.index_at_row(row);
    None
}

#[cfg(test)]
#[path = "mouse_tests.rs"]
mod tests;
