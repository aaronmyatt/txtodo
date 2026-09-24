//! The prompt bar (task `tui-revamp/tui-prompt`, c2 `c2-prompt.html:157-168`): Quick Add on every
//! screen. `Ctrl-Space` (or `Ctrl-Shift-Space` where the terminal tells them apart) gives it the
//! keyboard; Enter adds the line to this workspace's root list and toasts, an empty Enter or Esc
//! leaves. While it has the keyboard, chips edit the draft through core's chip edits (by click,
//! or `Alt` and the chip's key), and a strict-grammar hint shows once typing pauses.
//! Ref: <https://docs.rs/txtodo-core> (`chips::apply_chip`, `strict_hint::strict_hint`)

use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use txtodo_core::chips::{Chip, apply_chip};
use txtodo_proto::v1 as pb;

use crate::action::Action;
use crate::commands::{apply_of, today_local};
use crate::keymap::Command;
use crate::state::AppState;
use crate::state_nav::Focus;

/// How long typing must pause before the strict hint shows (the c2 mockup's 150 ms).
pub const HINT_AFTER: Duration = Duration::from_millis(150);

/// The chips, in the c2 order: their label, their `Alt` key and the chip.
pub const CHIPS: [(&str, char, &str); 9] = [
    ("(A)", 'a', "A"),
    ("(B)", 'b', "B"),
    ("(C)", 'c', "C"),
    ("x", 'x', "x"),
    ("+", 'p', "+"),
    ("@", 'o', "@"),
    ("due:", 'd', "due:"),
    ("t:", 't', "t:"),
    ("rec:", 'r', "rec:"),
];

/// Runs a prompt command; `None` for any other command.
pub fn run(state: &mut AppState, command: Command) -> Option<Option<Action>> {
    Some(match command {
        Command::PromptFocus => {
            state.nav.focus = Focus::Prompt;
            None
        }
        Command::PromptSubmit => submit(state),
        Command::PromptCancel => {
            state.nav.focus = Focus::List;
            None
        }
        _ => return None,
    })
}

/// Enter: adds the draft to the root list, clears it and keeps the keyboard for the next one; an
/// empty draft leaves the bar.
fn submit(state: &mut AppState) -> Option<Action> {
    let line = state.shell.prompt.buffer.trim().to_owned();
    if line.is_empty() {
        state.nav.focus = Focus::List;
        return None;
    }
    state.shell.prompt = crate::state::EditDraft::new_line();
    state.shell.prompt_typed_at = None;
    let add = pb::Mutation {
        kind: Some(pb::mutation::Kind::Add(pb::Add { line })),
    };
    let name = crate::ui::header::workspace_name(state);
    state.shell.pending_toast = Some(format!("Added to {name}"));
    Some(Action::Apply(apply_of(state, add)))
}

/// Applies chip `index` of [`CHIPS`] to the draft at its caret.
pub fn apply(state: &mut AppState, index: usize) {
    let Some(chip) = CHIPS.get(index).and_then(|(_, _, name)| Chip::parse(name)) else {
        return;
    };
    let draft = &mut state.shell.prompt;
    let (text, caret) = apply_chip(&draft.buffer, draft.caret, chip, &today_local());
    draft.buffer = text;
    draft.caret = caret;
    state.shell.prompt_typed_at = Some(Instant::now());
}

/// A key while the bar has the keyboard (its bindings, Enter and Esc, are resolved before this):
/// `Alt` and a chip's key applies the chip, anything else edits the draft.
pub fn on_key(state: &mut AppState, key: KeyEvent, now: Instant) {
    if key.modifiers.contains(KeyModifiers::ALT)
        && let KeyCode::Char(c) = key.code
        && let Some(index) = CHIPS.iter().position(|(_, k, _)| *k == c)
    {
        apply(state, index);
        return;
    }
    if crate::ui::edit::on_key(&mut state.shell.prompt, key) {
        state.shell.prompt_typed_at = Some(now);
    }
}

/// The strict hint for the draft, once typing has paused [`HINT_AFTER`].
pub fn hint(state: &AppState, now: Instant) -> Option<&'static str> {
    let typed = state.shell.prompt_typed_at?;
    if now.duration_since(typed) < HINT_AFTER {
        return None;
    }
    txtodo_core::strict_hint::strict_hint(&state.shell.prompt.buffer)
}

/// When the loop should wake to show the hint: [`HINT_AFTER`] past the last key, while the bar
/// has the keyboard.
pub fn hint_due(state: &AppState) -> Option<Instant> {
    (state.nav.focus == Focus::Prompt)
        .then_some(state.shell.prompt_typed_at?)
        .map(|at| at + HINT_AFTER)
        .filter(|due| *due > Instant::now())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn typed(state: &mut AppState, text: &str) {
        for c in text.chars() {
            on_key(
                state,
                KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE),
                Instant::now(),
            );
        }
    }

    #[test]
    fn enter_adds_to_the_root_list_and_toasts_the_workspace() {
        let mut state = AppState::fixture();
        state.workspace_label = Some("notes".to_owned());
        run(&mut state, Command::PromptFocus);
        typed(&mut state, "buy milk");
        let Some(Some(Action::Apply(req))) = run(&mut state, Command::PromptSubmit) else {
            panic!("Enter adds");
        };
        assert_eq!(req.path, "todo.txt");
        assert!(matches!(
            &req.mutations[0].kind,
            Some(pb::mutation::Kind::Add(add)) if add.line == "buy milk"
        ));
        assert_eq!(state.shell.pending_toast.as_deref(), Some("Added to notes"));
        assert_eq!(state.shell.prompt.buffer, "", "cleared for the next one");
        assert_eq!(state.nav.focus, Focus::Prompt, "and keeps the keyboard");
        assert_eq!(run(&mut state, Command::PromptSubmit), Some(None));
        assert_eq!(state.nav.focus, Focus::List, "an empty Enter leaves");
    }

    #[test]
    fn alt_and_a_chip_key_edits_through_core() {
        let mut state = AppState::fixture();
        typed(&mut state, "call mum");
        on_key(
            &mut state,
            KeyEvent::new(KeyCode::Char('a'), KeyModifiers::ALT),
            Instant::now(),
        );
        assert_eq!(state.shell.prompt.buffer, "(A) call mum");
        on_key(
            &mut state,
            KeyEvent::new(KeyCode::Char('o'), KeyModifiers::ALT),
            Instant::now(),
        );
        assert!(
            state.shell.prompt.buffer.ends_with('@'),
            "{}",
            state.shell.prompt.buffer
        );
    }

    #[test]
    fn the_hint_waits_for_a_pause() {
        let mut state = AppState::fixture();
        typed(&mut state, "(a) call mum");
        let typed_at = state.shell.prompt_typed_at.unwrap_or_else(Instant::now);
        assert_eq!(hint(&state, typed_at), None, "still typing");
        assert_eq!(
            hint(&state, typed_at + HINT_AFTER),
            Some(txtodo_core::strict_hint::LOWERCASE_PRIORITY)
        );
    }
}
