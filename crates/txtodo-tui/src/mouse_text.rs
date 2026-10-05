//! Clicks on text (task `tui-text-editing`): a click on a field puts the caret under the pointer, a
//! click outside the line being edited saves it, and a click on the text of the row that is
//! already selected starts editing it right there. Pure `AppState` work; `mouse` decides when.
//!
//! Columns map through the same code that drew the field (`ui::field`, `ui::notes_wrap`), so the
//! caret lands on the character under the pointer, wide characters and scrolling included.

use ratatui::layout::Rect;

use crate::action::Action;
use crate::commands;
use crate::hit::Field;
use crate::keymap::Command;
use crate::state::AppState;
use crate::state_detail::with_sub_list;
use crate::state_nav::Focus;
use crate::text_input;
use crate::ui::{field, notes_wrap, row};

/// Whether keys (and so the line editor) are on the detail panel's sub-list.
pub fn in_detail(state: &AppState) -> bool {
    state.nav.focus == Focus::Detail && state.detail.is_open()
}

/// A press on `which`, drawn over `area`, at cell (`x`, `y`): focuses the field and moves its caret
/// there.
pub fn press_field(state: &mut AppState, which: Field, area: Rect, x: u16, y: u16) {
    match which {
        Field::LineEdit => {
            if let Some(draft) = state.editing.as_mut() {
                draft.caret = field::offset_at(area, &draft.buffer, draft.caret, x);
            }
        }
        Field::Prompt => {
            let draft = &mut state.shell.prompt;
            draft.caret = field::offset_at(area, &draft.buffer, draft.caret, x);
        }
        Field::Parent => press_parent(state, area, x),
        Field::Notes => press_notes(state, area, x, y),
        Field::Search => {
            let shell = &mut state.shell;
            place(&shell.search, &mut shell.search_caret, area, x);
        }
        Field::Command => {
            if let Some(cmd) = &state.command {
                place(cmd, &mut state.command_caret, area, x);
            }
        }
        Field::SettingsFilter => {
            let settings = &mut state.settings;
            place(&settings.filter, &mut settings.filter_caret, area, x);
        }
        Field::SettingsField => {
            let settings = &mut state.settings;
            if let Some(text) = &settings.field {
                place(text, &mut settings.field_caret, area, x);
            }
        }
    }
}

/// Moves a tail-kept caret to the click at `x` in a field drawn over `area`.
fn place(text: &str, tail: &mut text_input::TailCaret, area: Rect, x: u16) {
    let caret = field::offset_at(area, text, tail.at(text), x);
    tail.set(text, caret);
}

/// The parent line: a click on the painted line opens the field with the caret under the pointer
/// (the painted line is not scrolled); a click on the open field moves its caret.
fn press_parent(state: &mut AppState, area: Rect, x: u16) {
    let open = state.detail.top().is_some_and(|l| l.parent_draft.is_some());
    if !open {
        commands::run(state, Command::DetailEditParent);
    }
    let Some(draft) = state.detail.top_mut().and_then(|l| l.parent_draft.as_mut()) else {
        return;
    };
    draft.caret = if open {
        field::offset_at(area, &draft.buffer, draft.caret, x)
    } else {
        text_input::offset_at_column(&draft.buffer, usize::from(x.saturating_sub(area.x)))
    };
}

/// The notes: gives them the keyboard, then the caret goes to the wrapped row and column clicked,
/// counted from the row the view starts at (the same rule `detail::draw_notes` scrolls by).
fn press_notes(state: &mut AppState, area: Rect, x: u16, y: u16) {
    commands::run(state, Command::DetailEditNotes);
    let Some(level) = state.detail.top_mut() else {
        return;
    };
    let notes = &mut level.notes;
    let width = match notes.wrap_width.get() {
        0 => area.width.saturating_sub(2).max(1),
        w => w,
    };
    let rows = notes_wrap::rows(&notes.text, usize::from(width));
    let (caret_row, _) = notes_wrap::caret_cell(&notes.text, &rows, notes.caret);
    let top = caret_row.saturating_sub(usize::from(area.height.max(1)) - 1);
    let row = top + usize::from(y.saturating_sub(area.y));
    // One cell of margin before the text.
    let column = usize::from(x.saturating_sub(area.x)).saturating_sub(1);
    notes.caret = notes_wrap::offset_at(&notes.text, &rows, row, column);
}

/// A press anywhere but the line being edited: saves it, as Enter would, and the press does
/// nothing else (the save is the one action this event returns).
pub fn save_edit(state: &mut AppState) -> Option<Action> {
    if in_detail(state) {
        return with_sub_list(state, |s| commands::run(s, Command::EditCommit)).flatten();
    }
    commands::run(state, Command::EditCommit)
}

/// The second press of a double-click on a row whose edit a click just started, untouched: the
/// user meant to open it, so the edit goes and the row opens.
pub fn open_instead(state: &mut AppState) -> Option<Action> {
    commands::run(state, Command::EditCancel);
    if in_detail(state) {
        return with_sub_list(state, |s| commands::run(s, Command::DetailOpen)).flatten();
    }
    commands::run(state, Command::DetailOpen)
}

/// Starts editing the selected row of the open list (root, or the sub-list when `sub`), with the
/// caret under screen column `x`; `left` is where the list's rows start. Returns the draft's text
/// at the start, so a double-click can tell it was not typed in.
pub fn edit_at(state: &mut AppState, sub: bool, left: u16, x: u16) -> Option<String> {
    let start = |s: &mut AppState| {
        commands::run(s, Command::ListEditStart);
        let gutter = if s.settings.prefs.line_numbers {
            u16::try_from(row::gutter_width(s)).unwrap_or(0)
        } else {
            0
        };
        let column = usize::from(x.saturating_sub(left + gutter));
        let draft = s.editing.as_mut()?;
        draft.caret = text_input::offset_at_column(&draft.buffer, column);
        Some(draft.buffer.clone())
    };
    if sub {
        with_sub_list(state, start).flatten()
    } else {
        start(state)
    }
}

#[cfg(test)]
#[path = "mouse_text_tests.rs"]
mod tests;
