//! Readline's emacs-mode keys for every text field (task `tui-readline-keys`): pure edits on a
//! `String` and a byte caret, the same pair every field keeps (`text_input`). Motions by line and by
//! word, kills into one shared slot and the yank back, transposes, word case, and a per-field undo
//! stack. Words are runs of letters and digits by UAX #29 word bounds, as readline's are; `Ctrl-W`
//! alone counts whitespace-separated words (readline's `unix-word-rubout`).
//! Refs: <https://tiswww.case.edu/php/chet/readline/readline.html#Commands-For-Moving>,
//! <https://tiswww.case.edu/php/chet/readline/readline.html#Commands-For-Killing>,
//! <https://docs.rs/unicode-segmentation/1.13/unicode_segmentation/trait.UnicodeSegmentation.html#tymethod.split_word_bound_indices>

use unicode_segmentation::UnicodeSegmentation;

use crate::text_input;

/// Where the logical line holding `caret` starts (after the previous `\n`).
pub fn line_start(text: &str, caret: usize) -> usize {
    text[..caret].rfind('\n').map_or(0, |i| i + 1)
}

/// Where the logical line holding `caret` ends (before its `\n`).
pub fn line_end(text: &str, caret: usize) -> usize {
    text[caret..].find('\n').map_or(text.len(), |i| caret + i)
}

/// Whether a word-bound segment is a word: it holds a letter or a digit.
fn is_word(segment: &str) -> bool {
    segment.chars().any(char::is_alphanumeric)
}

/// `Alt-F`: the end of the word the caret is in, or of the next one.
pub fn word_forward(text: &str, caret: usize) -> usize {
    text.split_word_bound_indices()
        .map(|(i, s)| (i, i + s.len(), s))
        .find(|&(_, end, s)| end > caret && is_word(s))
        .map_or(text.len(), |(_, end, _)| end)
}

/// `Alt-B`: the start of the word the caret is in (past its first character), or of the one
/// before.
pub fn word_back(text: &str, caret: usize) -> usize {
    text.split_word_bound_indices()
        .rev()
        .find(|&(i, s)| i < caret && is_word(s))
        .map_or(0, |(i, _)| i)
}

/// `Ctrl-W`'s stop: back over spaces, then back to the previous space (`unix-word-rubout`).
pub fn space_word_back(text: &str, caret: usize) -> usize {
    let before = text[..caret].trim_end_matches([' ', '\t']);
    before.rfind([' ', '\t', '\n']).map_or(0, |i| i + 1)
}

/// The one kill slot every field shares (`Ctrl-Y` yanks it). Kills right after one another join
/// up, as readline's do: a backward kill goes in front, a forward one behind.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Kill {
    /// The killed text.
    pub text: String,
    /// The last edit was a kill, so the next one joins it.
    pub joining: bool,
}

impl Kill {
    /// Takes in `killed`, cut from before the caret when `backward`.
    fn take(&mut self, killed: &str, backward: bool) {
        if !self.joining {
            self.text.clear();
        } else if backward {
            self.text.insert_str(0, killed);
            return;
        }
        self.text.push_str(killed);
    }
}

/// Cuts `from..to` out of `text` into `kill`, leaving the caret at `from`.
fn cut(text: &mut String, caret: &mut usize, kill: &mut Kill, from: usize, to: usize) {
    if from >= to {
        return;
    }
    let backward = to <= *caret;
    kill.take(&text[from..to], backward);
    text.replace_range(from..to, "");
    *caret = from;
}

/// Readline's edits, one per key; see the module doc for which key is which.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Op {
    /// `Ctrl-A`.
    LineStart,
    /// `Ctrl-E`.
    LineEnd,
    /// `Alt-B`, `Alt-Left`.
    WordBack,
    /// `Alt-F`, `Alt-Right`.
    WordForward,
    /// `Ctrl-B`.
    CharBack,
    /// `Ctrl-F`.
    CharForward,
    /// `Ctrl-D`: delete the character under the caret.
    DeleteForward,
    /// `Ctrl-H`: delete the character before the caret.
    DeleteBack,
    /// `Ctrl-W`: kill back to the previous space.
    KillSpaceWordBack,
    /// `Alt-Backspace`: kill back to the start of the word.
    KillWordBack,
    /// `Alt-D`: kill to the end of the word.
    KillWordForward,
    /// `Ctrl-U`: kill back to the start of the line.
    KillLineBack,
    /// `Ctrl-K`: kill to the end of the line; at its end, the line break.
    KillLineForward,
    /// `Ctrl-Y`: insert the kill slot.
    Yank,
    /// `Ctrl-T`: swap the characters around the caret.
    TransposeChars,
    /// `Alt-T`: swap the word before the caret with the one after.
    TransposeWords,
    /// `Alt-U`.
    Upcase,
    /// `Alt-L`.
    Downcase,
    /// `Alt-C`.
    Capitalize,
}

impl Op {
    /// Whether this op kills (so the next kill joins it).
    fn kills(self) -> bool {
        matches!(
            self,
            Op::KillSpaceWordBack
                | Op::KillWordBack
                | Op::KillWordForward
                | Op::KillLineBack
                | Op::KillLineForward
        )
    }
}

/// Applies `op` to a field. Undo is the caller's: it snapshots the field around this.
pub fn apply(op: Op, text: &mut String, caret: &mut usize, kill: &mut Kill) {
    let at = *caret;
    match op {
        Op::LineStart => *caret = line_start(text, at),
        Op::LineEnd => *caret = line_end(text, at),
        Op::WordBack => *caret = word_back(text, at),
        Op::WordForward => *caret = word_forward(text, at),
        Op::CharBack => text_input::left(text, caret),
        Op::CharForward => text_input::right(text, caret),
        Op::DeleteForward => {
            text_input::delete(text, at);
        }
        Op::DeleteBack => {
            text_input::backspace(text, caret);
        }
        Op::KillSpaceWordBack => cut(text, caret, kill, space_word_back(text, at), at),
        Op::KillWordBack => cut(text, caret, kill, word_back(text, at), at),
        Op::KillWordForward => {
            let end = word_forward(text, at);
            cut(text, caret, kill, at, end);
        }
        Op::KillLineBack => cut(text, caret, kill, line_start(text, at), at),
        Op::KillLineForward => {
            let end = line_end(text, at);
            // At the end of a line, the line break goes (emacs), so Ctrl-K repeated empties a block.
            let end = if end == at && at < text.len() {
                at + 1
            } else {
                end
            };
            cut(text, caret, kill, at, end);
        }
        Op::Yank => {
            text.insert_str(at, &kill.text);
            *caret = at + kill.text.len();
        }
        Op::TransposeChars => transpose_chars(text, caret),
        Op::TransposeWords => transpose_words(text, caret),
        Op::Upcase | Op::Downcase | Op::Capitalize => change_case(op, text, caret),
    }
    kill.joining = op.kills();
}

/// Readline's `transpose-chars`: the character before the caret swaps with the one under it and the
/// caret moves past both; at the end of a line, the last two swap.
fn transpose_chars(text: &mut String, caret: &mut usize) {
    let end = line_end(text, *caret);
    let at = if *caret == end {
        text_input::prev_boundary(text, *caret).unwrap_or(*caret)
    } else {
        *caret
    };
    let (Some(before), Some(after)) = (
        text_input::prev_boundary(text, at),
        text_input::next_boundary(text, at),
    ) else {
        return;
    };
    if before < line_start(text, at) || after > end {
        return;
    }
    let swapped = format!("{}{}", &text[at..after], &text[before..at]);
    text.replace_range(before..after, &swapped);
    *caret = after;
}

/// Readline's `transpose-words`: the word before the caret swaps with the word after it, the caret
/// lands after both. Nothing when there are not two words to swap.
fn transpose_words(text: &mut String, caret: &mut usize) {
    let words: Vec<(usize, usize)> = text
        .split_word_bound_indices()
        .filter(|(_, s)| is_word(s))
        .map(|(i, s)| (i, i + s.len()))
        .collect();
    // The second word is the first one ending after the caret; at the end, the last one.
    let second = words
        .iter()
        .position(|&(_, end)| end > *caret)
        .unwrap_or(words.len().saturating_sub(1));
    let (Some(&(a0, a1)), Some(&(b0, b1))) = (
        second.checked_sub(1).and_then(|i| words.get(i)),
        words.get(second),
    ) else {
        return;
    };
    let swapped = format!("{}{}{}", &text[b0..b1], &text[a1..b0], &text[a0..a1]);
    text.replace_range(a0..b1, &swapped);
    *caret = b1;
}

/// `Alt-U`/`Alt-L`/`Alt-C`: from the caret to the end of the word, upper, lower, or capitalized
/// (the first letter up, the rest down); the caret moves to the word's end.
fn change_case(op: Op, text: &mut String, caret: &mut usize) {
    let end = word_forward(text, *caret);
    let part = &text[*caret..end];
    let changed = match op {
        Op::Upcase => part.to_uppercase(),
        Op::Downcase => part.to_lowercase(),
        _ => {
            let mut out = String::with_capacity(part.len());
            let mut first = true;
            for c in part.chars() {
                if first && c.is_alphanumeric() {
                    out.extend(c.to_uppercase());
                    first = false;
                } else if first {
                    out.push(c);
                } else {
                    out.extend(c.to_lowercase());
                }
            }
            out
        }
    };
    let len = changed.len();
    text.replace_range(*caret..end, &changed);
    *caret += len;
}

/// One field's undo steps (`Ctrl-_`), oldest first, since it took the keyboard. A run of typed
/// characters is one step, as in readline.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Undo {
    steps: Vec<(String, usize)>,
    typing: bool,
}

impl Undo {
    /// Notes the field as it was before an edit that changed it; `typed` for a plain character.
    pub fn record(&mut self, text: &str, caret: usize, typed: bool) {
        if !(typed && self.typing) {
            self.steps.push((text.to_owned(), caret));
        }
        self.typing = typed;
    }

    /// The field as it was before the last step, taken off the stack.
    pub fn undo(&mut self) -> Option<(String, usize)> {
        self.typing = false;
        self.steps.pop()
    }

    /// Forgets every step: the keyboard went to another field.
    pub fn clear(&mut self) {
        self.steps.clear();
        self.typing = false;
    }
}

#[cfg(test)]
#[path = "readline_tests.rs"]
mod tests;
