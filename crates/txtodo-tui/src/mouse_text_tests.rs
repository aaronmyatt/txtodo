//! `mouse_text.rs`'s tests, driven through `Mouse::on_event` over `AppState::fixture()` and
//! hand-drawn hit maps, as `mouse_tests.rs` does.

use std::time::Instant;

use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;

use super::*;
use crate::hit::Target;
use crate::mouse::{DOUBLE_CLICK, Mouse};
use crate::state_detail::{Level, Parent, Part};

fn event(kind: MouseEventKind, column: u16, row: u16) -> MouseEvent {
    MouseEvent {
        kind,
        column,
        row,
        modifiers: KeyModifiers::NONE,
    }
}

fn down(column: u16, row: u16) -> MouseEvent {
    event(MouseEventKind::Down(MouseButton::Left), column, row)
}

fn up(column: u16, row: u16) -> MouseEvent {
    event(MouseEventKind::Up(MouseButton::Left), column, row)
}

/// The fixture with its list drawn over the top ten rows.
fn listed() -> AppState {
    let mut state = AppState::fixture();
    state
        .hits
        .record_list(Rect::new(0, 0, 60, 10), 0, state.row_count());
    state
}

/// Where row text starts: past the line-number gutter, when it is on.
fn text_left(state: &AppState) -> u16 {
    if state.settings.prefs.line_numbers {
        u16::try_from(row::gutter_width(state)).unwrap_or(0)
    } else {
        0
    }
}

#[test]
fn a_click_on_the_selected_rows_text_edits_it_with_the_caret_under_the_pointer() {
    let mut state = listed();
    let mut mouse = Mouse::default();
    let t0 = Instant::now();
    let x = text_left(&state) + 4; // the "2" of "(A) 2026-..."
    mouse.on_event(&mut state, down(x, 0), t0);
    assert!(state.editing.is_none(), "nothing until the release");
    mouse.on_event(&mut state, up(x, 0), t0);
    let draft = state.editing.as_ref().unwrap_or_else(|| panic!("editing"));
    assert_eq!(draft.caret, 4);
    assert_eq!(&draft.buffer[draft.caret..draft.caret + 4], "2026");
}

#[test]
fn a_click_on_an_unselected_row_only_selects_it() {
    let mut state = listed();
    let mut mouse = Mouse::default();
    let t0 = Instant::now();
    mouse.on_event(&mut state, down(8, 3), t0);
    mouse.on_event(&mut state, up(8, 3), t0);
    assert_eq!(state.cursor, 3);
    assert!(state.editing.is_none());
}

#[test]
fn a_double_click_on_the_selected_row_still_opens_it() {
    let mut state = listed();
    let mut mouse = Mouse::default();
    let t0 = Instant::now();
    mouse.on_event(&mut state, down(8, 0), t0);
    mouse.on_event(&mut state, up(8, 0), t0);
    // The edit the first click started is drawn as a field over the row.
    state.hits.push(
        Rect::new(2, 0, 58, 1),
        Target::Field(Field::LineEdit, Rect::new(2, 0, 58, 1)),
    );
    let action = mouse.on_event(&mut state, down(8, 0), t0 + DOUBLE_CLICK);
    assert!(
        matches!(action, Some(Action::OpenDetail(_))),
        "opens: {action:?}"
    );
    assert!(state.editing.is_none(), "the untouched edit is dropped");
}

#[test]
fn while_editing_a_click_on_the_line_moves_the_caret_and_one_elsewhere_saves() {
    let mut state = listed();
    let mut mouse = Mouse::default();
    // Caret at the start, so the field is not scrolled (the line is longer than the field).
    commands::run(&mut state, Command::ListEditStart);
    let field = Rect::new(2, 0, 58, 1);
    state
        .hits
        .push(field, Target::Field(Field::LineEdit, field));
    let t0 = Instant::now();
    mouse.on_event(&mut state, down(2 + 3, 0), t0);
    assert_eq!(state.editing.as_ref().map(|d| d.caret), Some(3));
    if let Some(draft) = state.editing.as_mut() {
        draft.buffer.insert(3, '!');
    }
    let action = mouse.on_event(&mut state, down(5, 6), t0 + DOUBLE_CLICK * 2);
    assert!(
        matches!(action, Some(Action::Apply(_))),
        "saved: {action:?}"
    );
    assert!(state.editing.is_none());
}

/// The fixture with one detail level open and its parent and notes areas drawn.
fn with_panel() -> (AppState, Rect, Rect) {
    let mut state = listed();
    state.detail.levels.push(Level {
        parent: Parent {
            line_number: 1,
            raw: "Water the plants".to_owned(),
            ..Parent::default()
        },
        ..Level::default()
    });
    state.detail.levels[0].notes.text = "ferns first\nthen moss".to_owned();
    let parent = Rect::new(9, 12, 40, 1);
    let notes = Rect::new(0, 15, 40, 4);
    state
        .hits
        .push(parent, Target::Field(Field::Parent, parent));
    state.hits.push(notes, Target::Field(Field::Notes, notes));
    (state, parent, notes)
}

#[test]
fn a_click_on_the_parent_line_opens_it_with_the_caret_under_the_pointer() {
    let (mut state, parent, _) = with_panel();
    Mouse::default().on_event(&mut state, down(parent.x + 6, parent.y), Instant::now());
    assert_eq!(state.nav.focus, Focus::Detail);
    assert_eq!(state.detail.part, Part::Parent);
    let caret = state
        .detail
        .top()
        .and_then(|l| l.parent_draft.as_ref())
        .map(|d| d.caret);
    assert_eq!(caret, Some(6), "before \"the\"");
}

#[test]
fn a_click_in_the_notes_gives_them_the_keyboard_and_the_caret() {
    let (mut state, _, notes) = with_panel();
    // Row 2 of the notes area: past the one-cell margin, three cells in, after "the".
    Mouse::default().on_event(
        &mut state,
        down(notes.x + 1 + 3, notes.y + 1),
        Instant::now(),
    );
    assert_eq!(state.nav.focus, Focus::Detail);
    assert_eq!(state.detail.part, Part::Notes);
    let level = state.detail.top().unwrap_or_else(|| panic!("a level"));
    assert_eq!(&level.notes.text[level.notes.caret..], "n moss");
}

#[test]
fn a_click_in_the_search_or_the_command_line_moves_its_caret() {
    let mut state = listed();
    state.nav.focus = Focus::Search;
    state.shell.search = "hello".to_owned();
    let search = Rect::new(10, 0, 20, 1);
    press_field(&mut state, Field::Search, search, 12, 0);
    assert_eq!(state.shell.search_caret.at(&state.shell.search), 2);
    state.command = Some("w home".to_owned());
    press_field(&mut state, Field::Command, Rect::new(1, 9, 30, 1), 2, 9);
    let cmd = state.command.clone().unwrap_or_default();
    assert_eq!(state.command_caret.at(&cmd), 1);
}
