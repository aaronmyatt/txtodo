//! `fields.rs`'s tests, driven through `Input` as the event loop does.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::*;
use crate::input::Input;

fn press(state: &mut AppState, input: &mut Input, code: KeyCode, mods: KeyModifiers) {
    input.on_key(state, KeyEvent::new(code, mods));
}

fn ctrl(c: char) -> (KeyCode, KeyModifiers) {
    (KeyCode::Char(c), KeyModifiers::CONTROL)
}

fn alt(c: char) -> (KeyCode, KeyModifiers) {
    (KeyCode::Char(c), KeyModifiers::ALT)
}

fn keys(state: &mut AppState, input: &mut Input, keys: &[(KeyCode, KeyModifiers)]) {
    for (code, mods) in keys {
        press(state, input, *code, *mods);
    }
}

fn typed(text: &str) -> Vec<(KeyCode, KeyModifiers)> {
    text.chars()
        .map(|c| (KeyCode::Char(c), KeyModifiers::NONE))
        .collect()
}

#[test]
fn the_line_editor_takes_motions_kills_and_yanks() {
    let mut state = AppState::fixture();
    let mut input = Input::default();
    state.move_first(); // the Add-a-line row
    keys(
        &mut state,
        &mut input,
        &[(KeyCode::Char('i'), KeyModifiers::NONE)],
    );
    keys(&mut state, &mut input, &typed("call mum"));
    keys(&mut state, &mut input, &[ctrl('a'), alt('f'), ctrl('k')]);
    let draft = state.editing.clone().unwrap_or_else(|| panic!("editing"));
    assert_eq!((draft.buffer.as_str(), draft.caret), ("call", 4));
    keys(&mut state, &mut input, &[ctrl('a'), ctrl('y')]);
    assert_eq!(state.editing.map(|d| d.buffer), Some(" mumcall".to_owned()));
}

#[test]
fn the_prompt_bar_and_search_take_them_too() {
    let mut state = AppState::fixture();
    let mut input = Input::default();
    state.nav.focus = Focus::Prompt;
    keys(&mut state, &mut input, &typed("buy oat milk"));
    keys(&mut state, &mut input, &[alt('b'), alt('b'), ctrl('w')]);
    assert_eq!(state.shell.prompt.buffer, "oat milk");
    state.nav.focus = Focus::Search;
    keys(&mut state, &mut input, &typed("plants"));
    keys(&mut state, &mut input, &[ctrl('a'), alt('u')]);
    assert_eq!(state.shell.search, "PLANTS");
}

#[test]
fn ctrl_underscore_undoes_the_fields_last_edit_and_a_typing_run_at_once() {
    let mut state = AppState::fixture();
    let mut input = Input::default();
    state.nav.focus = Focus::Prompt;
    keys(&mut state, &mut input, &typed("call mum"));
    keys(&mut state, &mut input, &[alt('c'), ctrl('a'), alt('c')]);
    assert_eq!(state.shell.prompt.buffer, "Call mum");
    // Legacy terminals send Ctrl-_ as Ctrl-7.
    keys(&mut state, &mut input, &[ctrl('7')]);
    assert_eq!(state.shell.prompt.buffer, "call mum");
    keys(&mut state, &mut input, &[ctrl('_')]);
    assert_eq!(state.shell.prompt.buffer, "", "the typing run in one step");
}

#[test]
fn the_kill_slot_is_shared_between_fields_and_list_mode_ignores_the_keys() {
    let mut state = AppState::fixture();
    let mut input = Input::default();
    state.nav.focus = Focus::Search;
    keys(&mut state, &mut input, &typed("ferns"));
    keys(&mut state, &mut input, &[ctrl('u')]);
    state.nav.focus = Focus::Prompt;
    keys(&mut state, &mut input, &[ctrl('y')]);
    assert_eq!(state.shell.prompt.buffer, "ferns");
    state.nav.focus = Focus::List;
    let cursor = state.cursor;
    keys(&mut state, &mut input, &[ctrl('a'), alt('f'), ctrl('k')]);
    assert_eq!(state.cursor, cursor, "no field: nothing");
}
