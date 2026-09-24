//! `mouse.rs`'s tests, over `AppState::fixture()` (rows: 0 a task, 1 a done task, 2 blank, 3 a
//! task, 4 the Add-a-line row) and a hand-drawn hit map.

use super::*;
use crossterm::event::KeyModifiers;
use ratatui::layout::Rect;
use txtodo_proto::v1 as pb;

fn at(kind: MouseEventKind, row: u16) -> MouseEvent {
    MouseEvent {
        kind,
        column: 5,
        row,
        modifiers: KeyModifiers::NONE,
    }
}

fn down(row: u16) -> MouseEvent {
    at(MouseEventKind::Down(MouseButton::Left), row)
}

fn up(row: u16) -> MouseEvent {
    at(MouseEventKind::Up(MouseButton::Left), row)
}

/// The fixture with a hand-drawn hit map: the list over `height` rows from the top, no overlays.
fn fixture(height: u16) -> AppState {
    let mut state = AppState::fixture();
    state
        .hits
        .record_list(Rect::new(0, 0, 40, height), 0, state.row_count());
    state
}

/// The `MoveBefore` anchor's line number, or `None` for a `MoveToEnd`.
fn anchor(action: Option<Action>) -> Option<u32> {
    let Some(Action::Apply(req)) = action else {
        panic!("expected an apply, got {action:?}");
    };
    match req.mutations[0].kind.clone() {
        Some(pb::mutation::Kind::MoveBefore(m)) => m.before.map(|b| b.line_number),
        Some(pb::mutation::Kind::MoveToEnd(_)) => None,
        other => panic!("not a move: {other:?}"),
    }
}

#[test]
fn a_click_selects_the_row_and_a_quick_second_one_opens_its_detail() {
    let mut state = fixture(10);
    let mut mouse = Mouse::default();
    let t0 = Instant::now();
    assert_eq!(mouse.on_event(&mut state, down(3), t0), None);
    assert_eq!(state.cursor, 3);
    mouse.on_event(&mut state, up(3), t0);
    let action = mouse.on_event(&mut state, down(3), t0 + DOUBLE_CLICK);
    let Some(Action::OpenDetail(parent)) = action else {
        panic!("a double-click opens the detail: {action:?}");
    };
    assert_eq!(parent.line_number, 4);
}

#[test]
fn a_slow_second_click_or_one_on_another_row_is_just_a_click() {
    let mut state = fixture(10);
    let mut mouse = Mouse::default();
    let t0 = Instant::now();
    mouse.on_event(&mut state, down(0), t0);
    mouse.on_event(
        &mut state,
        down(0),
        t0 + DOUBLE_CLICK + Duration::from_millis(1),
    );
    assert!(!state.detail.is_open(), "too slow");
    mouse.on_event(
        &mut state,
        down(1),
        t0 + DOUBLE_CLICK + Duration::from_millis(2),
    );
    assert!(state.editing.is_none(), "another row");
    assert_eq!(state.cursor, 1);
}

#[test]
fn the_wheel_scrolls_the_view_and_pulls_the_cursor_along() {
    let mut state = fixture(2);
    let mut mouse = Mouse::default();
    let now = Instant::now();
    mouse.on_event(&mut state, at(MouseEventKind::ScrollDown, 0), now);
    assert_eq!(
        state.scroll, 3,
        "clamped to the last full view (5 rows, 2 high)"
    );
    assert_eq!(state.cursor, 3, "pulled into view");
    mouse.on_event(&mut state, at(MouseEventKind::ScrollUp, 0), now);
    assert_eq!((state.scroll, state.cursor), (0, 1));
}

#[test]
fn the_pointer_hovers_a_row_and_leaving_the_list_clears_it() {
    let mut state = fixture(10);
    let mut mouse = Mouse::default();
    let now = Instant::now();
    mouse.on_event(&mut state, at(MouseEventKind::Moved, 1), now);
    assert_eq!(state.hover, Some(1));
    mouse.on_event(&mut state, at(MouseEventKind::Moved, 9), now);
    assert_eq!(state.hover, None, "no row there");
}

#[test]
fn dragging_a_row_moves_it_to_where_it_is_dropped() {
    let now = Instant::now();
    let drag = |from: u16, to: u16| {
        let mut state = fixture(10);
        let mut mouse = Mouse::default();
        mouse.on_event(&mut state, down(from), now);
        let action = mouse.on_event(&mut state, up(to), now);
        (action, state.cursor)
    };
    let (action, cursor) = drag(3, 0);
    assert_eq!((anchor(action), cursor), (Some(1), 0), "up: before row 0");
    let (action, cursor) = drag(0, 1);
    assert_eq!(
        (anchor(action), cursor),
        (Some(4), 2),
        "down: before the task after row 1"
    );
    let (action, _) = drag(0, 2);
    assert_eq!(
        anchor(action),
        Some(4),
        "a blank drop row counts as the task above it"
    );
    let (action, cursor) = drag(0, 4);
    assert_eq!(
        (anchor(action), cursor),
        (None, 3),
        "the Add-a-line row is the end"
    );
    assert_eq!(drag(3, 3).0, None, "dropped where it started");
    assert_eq!(drag(2, 0).0, None, "a blank line is never moved");
    assert_eq!(drag(3, 2).0, None, "only blanks between: no move");
}

#[test]
fn overlays_and_text_fields_keep_the_mouse_off_the_list() {
    let mut state = fixture(10);
    state.hits.push(Rect::new(0, 9, 40, 1), Target::Inert);
    let mut mouse = Mouse::default();
    let now = Instant::now();
    state.cursor = 4;
    mouse.on_event(&mut state, down(9), now);
    assert_eq!(state.cursor, 4, "an overlay takes the click");
    state.editing = Some(crate::state::EditDraft::new_line());
    mouse.on_event(&mut state, down(0), now);
    assert_eq!(state.cursor, 4, "the editor has the screen");
}

#[test]
fn a_clickable_runs_its_command() {
    let mut state = fixture(10);
    state
        .hits
        .push(Rect::new(0, 0, 10, 1), Target::Command(Command::SyncOpen));
    Mouse::default().on_event(&mut state, down(0), Instant::now());
    assert!(state.sync_visible);
}
