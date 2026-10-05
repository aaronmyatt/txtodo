//! `detail.rs`'s tests: the panel drawn on a test terminal.

use super::*;
use crate::state::LineState;
use crate::state_detail::{Doc, Parent};
use ratatui::Terminal;
use ratatui::backend::TestBackend;

fn drawn(state: &AppState) -> (Vec<String>, HitMap) {
    let mut terminal = Terminal::new(TestBackend::new(60, 12)).unwrap_or_else(|e| panic!("{e}"));
    let mut hits = HitMap::default();
    terminal
        .draw(|f| draw(f, f.area(), state, &mut hits))
        .unwrap_or_else(|e| panic!("{e}"));
    let rows = terminal
        .backend()
        .buffer()
        .content()
        .chunks(60)
        .map(|r| r.iter().map(|c| c.symbol()).collect())
        .collect();
    (rows, hits)
}

#[test]
fn the_panel_shows_crumbs_parent_sub_list_and_notes() {
    let mut state = AppState::fixture();
    state.shell.root = "/home/u/notes".to_owned();
    state.detail.levels.push(Level {
        parent: Parent {
            path: "todo.txt".to_owned(),
            line_number: 4,
            raw: "Water the plants +home @garden".to_owned(),
            ..Parent::default()
        },
        dir: "tasks/plants".to_owned(),
        doc: Doc {
            path: "tasks/plants/todo.txt".to_owned(),
            lines: vec![LineState::from_raw(1, "x fill the can")],
            ..Doc::default()
        },
        ..Level::default()
    });
    state.detail.levels[0].notes.text = "ferns first".to_owned();
    let (rows, hits) = drawn(&state);
    let all = rows.join("\n");
    assert!(
        rows[1].contains("notes \u{203a}  Water the plants +home"),
        "{all}"
    );
    assert!(rows[2].contains("Parent  Water the plants"), "{all}");
    assert!(
        rows[2].contains("Mark done"),
        "every sub-task is done: {all}"
    );
    assert!(all.contains("1 x fill the can"), "{all}");
    assert!(all.contains("ferns first"), "{all}");
    assert_eq!(hits.at(2, 1), Some(Target::Command(Command::DetailClose)));
    assert_eq!(hits.at(20, 1), Some(Target::Crumb(1)));
    let sub_row = u16::try_from(
        rows.iter()
            .position(|r| r.contains("fill the can"))
            .unwrap_or(0),
    )
    .unwrap_or(0);
    assert_eq!(hits.at(5, sub_row), Some(Target::DetailRow(0)));
}

/// Draws `state` and returns where the terminal cursor ended up, `None` when it is hidden.
fn cursor(state: &AppState) -> Option<ratatui::layout::Position> {
    let mut terminal = Terminal::new(TestBackend::new(60, 12)).unwrap_or_else(|e| panic!("{e}"));
    let mut hits = HitMap::default();
    terminal
        .draw(|f| draw(f, f.area(), state, &mut hits))
        .unwrap_or_else(|e| panic!("{e}"));
    // Ref: https://docs.rs/ratatui/0.30/ratatui/backend/struct.TestBackend.html#method.cursor_visible
    let backend = terminal.backend();
    backend.cursor_visible().then(|| backend.cursor_position())
}

fn one_level(state: &mut AppState) {
    state.detail.levels.push(Level {
        parent: Parent {
            line_number: 1,
            raw: "Water the plants".to_owned(),
            ..Parent::default()
        },
        ..Level::default()
    });
}

#[test]
fn the_parent_field_puts_the_real_cursor_on_its_caret() {
    let mut state = AppState::fixture();
    one_level(&mut state);
    state.nav.focus = crate::state_nav::Focus::Detail;
    state.detail.part = Part::Parent;
    let line = LineState::from_raw(1, "Water the plants");
    let mut draft = crate::state::EditDraft::for_line(&line, true);
    draft.caret = "Water".len();
    state.detail.levels[0].parent_draft = Some(draft);
    // " Parent  " is nine cells; the panel's top border is row 0, crumbs row 1, parent row 2.
    assert_eq!(
        cursor(&state),
        Some(ratatui::layout::Position::new(9 + 5, 2))
    );
    let (rows, _) = drawn(&state);
    assert!(
        rows[2].contains("Parent \u{258c}Water the plants"),
        "{rows:?}"
    );
    state.nav.focus = crate::state_nav::Focus::List;
    assert_eq!(cursor(&state), None, "no cursor while keys go to the list");
}

#[test]
fn the_notes_put_the_real_cursor_on_their_caret() {
    let mut state = AppState::fixture();
    one_level(&mut state);
    state.nav.focus = crate::state_nav::Focus::Detail;
    state.detail.part = Part::Notes;
    state.detail.levels[0].notes.text = "ab\ncd".to_owned();
    state.detail.levels[0].notes.caret = 4; // after "c"
    let (rows, _) = drawn(&state);
    let first = rows.iter().position(|r| r.starts_with(" ab")).unwrap_or(0);
    let at = cursor(&state).unwrap_or_else(|| panic!("a cursor in the notes"));
    assert_eq!((at.x, usize::from(at.y)), (2, first + 1));
    assert!(!rows.join("").contains('\u{258f}'), "no drawn caret");
}

#[test]
fn the_panel_edge_lights_only_while_it_has_the_keyboard() {
    let mut state = AppState::fixture();
    one_level(&mut state);
    let edge_fg = |state: &AppState| {
        let mut terminal =
            Terminal::new(TestBackend::new(60, 12)).unwrap_or_else(|e| panic!("{e}"));
        let mut hits = HitMap::default();
        terminal
            .draw(|f| draw(f, f.area(), state, &mut hits))
            .unwrap_or_else(|e| panic!("{e}"));
        terminal.backend().buffer()[(30, 0)].fg
    };
    state.nav.focus = crate::state_nav::Focus::Detail;
    assert_eq!(edge_fg(&state), crate::theme::current().accent());
    state.nav.focus = crate::state_nav::Focus::List;
    assert_ne!(edge_fg(&state), crate::theme::current().accent());
}

#[test]
fn a_sub_line_is_edited_on_its_own_row_inside_the_panel() {
    let mut state = AppState::fixture();
    one_level(&mut state);
    state.detail.levels[0].doc = Doc {
        path: "tasks/plants/todo.txt".to_owned(),
        lines: vec![
            LineState::from_raw(1, "fill the can"),
            LineState::from_raw(2, "water ferns"),
        ],
        cursor: 1,
        ..Doc::default()
    };
    state.nav.focus = crate::state_nav::Focus::Detail;
    state.detail.part = Part::Sub;
    let line = LineState::from_raw(2, "water ferns");
    state.editing = Some(crate::state::EditDraft::for_line(&line, false));
    let (rows, _) = drawn(&state);
    let at = cursor(&state).unwrap_or_else(|| panic!("a cursor on the edited row"));
    let row = &rows[usize::from(at.y)];
    assert!(row.contains("\u{258c}water ferns"), "{rows:?}");
    assert!(!rows.join("").contains("edit >"), "no separate edit row");
}

#[test]
fn long_notes_wrap_and_the_cursor_follows_onto_the_second_row() {
    let mut state = AppState::fixture();
    one_level(&mut state);
    state.nav.focus = crate::state_nav::Focus::Detail;
    state.detail.part = Part::Notes;
    // The panel is 60 wide: 58 cells of text per row.
    let text = "word ".repeat(14);
    state.detail.levels[0].notes.caret = text.len();
    state.detail.levels[0].notes.text = text;
    let (rows, _) = drawn(&state);
    let first = rows
        .iter()
        .position(|r| r.starts_with(" word"))
        .unwrap_or(0);
    assert!(rows[first + 1].starts_with(" word"), "wrapped: {rows:?}");
    let at = cursor(&state).unwrap_or_else(|| panic!("a cursor"));
    // 14 words of 5 cells: 11 fit the first row (55 cells), 3 on the second, caret after them.
    assert_eq!((usize::from(at.y), at.x), (first + 1, 1 + 15));
}
