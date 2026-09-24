//! The `notes.md` editor (task `tui-revamp/tui-detail`): a multi-line text field with a caret.
//! Typing inserts, Enter breaks the line, Backspace and Delete remove, the arrows, Home and End
//! move. Pure edits on [`Notes`]; the panel draws it and `app_loop` saves it once typing pauses.
//! Ref: <https://doc.rust-lang.org/std/string/struct.String.html#method.insert>

use std::time::Instant;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::state_detail::Notes;

/// Applies one key to `notes` at `now`; `true` when it was an editing or caret key.
pub fn on_key(notes: &mut Notes, key: KeyEvent, now: Instant) -> bool {
    let plain = !key
        .modifiers
        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT);
    let typed = match key.code {
        KeyCode::Char(c) if plain => {
            insert(notes, c);
            true
        }
        KeyCode::Enter => {
            insert(notes, '\n');
            true
        }
        KeyCode::Backspace => backspace(notes),
        KeyCode::Delete => delete(notes),
        KeyCode::Left => return step(notes, false),
        KeyCode::Right => return step(notes, true),
        KeyCode::Up => return line_step(notes, false),
        KeyCode::Down => return line_step(notes, true),
        KeyCode::Home => {
            notes.caret = line_start(&notes.text, notes.caret);
            return true;
        }
        KeyCode::End => {
            notes.caret = line_end(&notes.text, notes.caret);
            return true;
        }
        _ => return false,
    };
    if typed {
        notes.typed_at = Some(now);
    }
    typed
}

fn insert(notes: &mut Notes, c: char) {
    notes.text.insert(notes.caret, c);
    notes.caret += c.len_utf8();
}

fn backspace(notes: &mut Notes) -> bool {
    let Some(prev) = notes.text[..notes.caret].chars().next_back() else {
        return false;
    };
    notes.caret -= prev.len_utf8();
    notes.text.remove(notes.caret);
    true
}

fn delete(notes: &mut Notes) -> bool {
    if notes.caret >= notes.text.len() {
        return false;
    }
    notes.text.remove(notes.caret);
    true
}

fn step(notes: &mut Notes, forward: bool) -> bool {
    let next = if forward {
        notes.text[notes.caret..]
            .chars()
            .next()
            .map(|c| notes.caret + c.len_utf8())
    } else {
        notes.text[..notes.caret]
            .chars()
            .next_back()
            .map(|c| notes.caret - c.len_utf8())
    };
    if let Some(at) = next {
        notes.caret = at;
    }
    true
}

/// The byte offset where the line holding `at` starts.
fn line_start(text: &str, at: usize) -> usize {
    text[..at].rfind('\n').map_or(0, |i| i + 1)
}

/// The byte offset where the line holding `at` ends (before its `\n`).
fn line_end(text: &str, at: usize) -> usize {
    text[at..].find('\n').map_or(text.len(), |i| at + i)
}

/// Up or Down: the same column on the line above or below, or its end when it is shorter.
fn line_step(notes: &mut Notes, down: bool) -> bool {
    let text = &notes.text;
    let start = line_start(text, notes.caret);
    let column = text[start..notes.caret].chars().count();
    let target = if down {
        let end = line_end(text, notes.caret);
        if end >= text.len() {
            return true;
        }
        end + 1
    } else {
        if start == 0 {
            return true;
        }
        line_start(text, start - 1)
    };
    let end = line_end(text, target);
    notes.caret = text[target..end]
        .char_indices()
        .nth(column)
        .map_or(end, |(i, _)| target + i);
    true
}

/// The caret as `(line, column)` in chars, for drawing it.
pub fn caret_position(notes: &Notes) -> (usize, usize) {
    let before = &notes.text[..notes.caret];
    let line = before.matches('\n').count();
    let column = before[line_start(&notes.text, notes.caret)..]
        .chars()
        .count();
    (line, column)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn press(notes: &mut Notes, code: KeyCode) {
        on_key(
            notes,
            KeyEvent::new(code, KeyModifiers::NONE),
            Instant::now(),
        );
    }

    fn typed(text: &str) -> Notes {
        let mut notes = Notes::default();
        for c in text.chars() {
            let code = if c == '\n' {
                KeyCode::Enter
            } else {
                KeyCode::Char(c)
            };
            press(&mut notes, code);
        }
        notes
    }

    #[test]
    fn typing_inserts_at_the_caret_and_marks_the_notes_dirty() {
        let mut notes = typed("ab\ncd");
        assert_eq!(notes.text, "ab\ncd");
        assert!(notes.dirty() && notes.typed_at.is_some());
        press(&mut notes, KeyCode::Left);
        press(&mut notes, KeyCode::Backspace);
        assert_eq!(notes.text, "ab\nd");
        assert_eq!(caret_position(&notes), (1, 0));
    }

    #[test]
    fn up_and_down_keep_the_column_or_stop_at_a_shorter_line_end() {
        let mut notes = typed("long line\nab\nlonger");
        assert_eq!(caret_position(&notes), (2, 6));
        press(&mut notes, KeyCode::Up);
        assert_eq!(caret_position(&notes), (1, 2), "ab is shorter");
        press(&mut notes, KeyCode::Up);
        assert_eq!(caret_position(&notes), (0, 2));
        press(&mut notes, KeyCode::Up);
        assert_eq!(caret_position(&notes), (0, 2), "the top line stays");
        press(&mut notes, KeyCode::End);
        press(&mut notes, KeyCode::Down);
        assert_eq!(caret_position(&notes), (1, 2));
    }

    #[test]
    fn caret_keys_do_not_count_as_typing() {
        let mut notes = Notes::default();
        press(&mut notes, KeyCode::Left);
        assert_eq!(notes.typed_at, None);
        press(&mut notes, KeyCode::Delete);
        assert_eq!(notes.typed_at, None, "nothing to delete");
    }
}
