//! Mouse events (task `tui-revamp/tui-mouse`), looked up in the last frame's [`crate::hit::HitMap`]: a click
//! selects a row or runs a clickable's [`Command`], a double-click edits the row, the wheel
//! scrolls the list, the pointer hovers a row, and dragging a row drops it somewhere else. Pure and
//! terminal-free, so it is tested over a fixed map.
//!
//! crossterm reports no double-click: two left presses on the same cell within [`DOUBLE_CLICK`]
//! make one. The mouse only acts in list mode; while a line is being edited, the `:` line is open
//! or a sheet is up, the keyboard has the screen.
//! Ref: <https://docs.rs/crossterm/latest/crossterm/event/struct.MouseEvent.html>

use std::time::{Duration, Instant};

use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};

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
            MouseEventKind::ScrollDown => scroll(state, WHEEL_ROWS),
            MouseEventKind::ScrollUp => scroll(state, -WHEEL_ROWS),
            MouseEventKind::Down(MouseButton::Left) => self.press(state, target, &ev, now),
            MouseEventKind::Up(MouseButton::Left) => self.release(state, target),
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
            Target::Inert => return None,
        };
        let double = self.last_press.is_some_and(|(column, line, at)| {
            (column, line) == (ev.column, ev.row) && now.duration_since(at) <= DOUBLE_CLICK
        });
        state.cursor = row;
        // A click on the list takes the keyboard back from the search field; the query stays.
        state.nav.focus = crate::state_nav::Focus::List;
        if double {
            self.last_press = None;
            self.dragging = None;
            // The detail panel takes this over once it exists (tasks/tui-revamp/tui-detail).
            return commands::run(state, Command::ListEditEnd);
        }
        self.last_press = Some((ev.column, ev.row, now));
        self.dragging = Some(row);
        None
    }

    fn release(&mut self, state: &mut AppState, target: Option<Target>) -> Option<Action> {
        let from = self.dragging.take()?;
        match target? {
            Target::Row(to) if to != from => commands::move_row(state, from, to),
            _ => None,
        }
    }
}

/// Whether something other than the list has the keyboard.
fn busy(state: &AppState) -> bool {
    state.editing.is_some() || state.command.is_some() || state.conflicts_open || state.offers.open
}

fn row_of(target: Option<Target>) -> Option<usize> {
    match target? {
        Target::Row(row) => Some(row),
        Target::Command(_) | Target::MenuItem(_) | Target::Inert => None,
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
    state.cursor = state.cursor.clamp(state.scroll, bottom);
    None
}

#[cfg(test)]
#[path = "mouse_tests.rs"]
mod tests;
