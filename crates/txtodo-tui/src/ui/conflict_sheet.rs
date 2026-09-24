//! The conflict review sheet (task `tui-revamp/tui-tasks`, replacing the `r` pane; desktop's
//! `ConflictReviewSheet`): a modal over the screen for the selected flag, "k of N", mine, theirs
//! and a merged preview marked as a char diff, and the three choices as buttons. The preview is
//! for reading only: "keep merged" keeps what the file already holds (design §4.7), it never sends
//! this text. The diff is core's `diff_text`, filled out to full runs the way `txtodo-ffi`'s
//! `diff_view.rs` does for desktop.
//! Ref: <https://docs.rs/ratatui/latest/ratatui/widgets/struct.Clear.html>

use ratatui::Frame;
use ratatui::layout::{Margin, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use txtodo_core::{TextEdit, diff_text};

use crate::hit::{HitMap, Target};
use crate::keymap::Command;
use crate::state::AppState;

/// The buttons, left to right: label and command.
const BUTTONS: [(&str, Command); 4] = [
    ("m Keep mine", Command::ConflictsKeepMine),
    ("t Keep theirs", Command::ConflictsKeepTheirs),
    ("M Keep merged", Command::ConflictsKeepMerged),
    ("Esc Close", Command::ConflictsClose),
];

/// `mine` → `theirs` as full-coverage runs: `(text, changed)` where `changed` is `Some(true)` for
/// text only in `theirs`, `Some(false)` for text only in `mine`, `None` for both.
pub fn merged_runs(mine: &str, theirs: &str) -> Vec<(String, Option<bool>)> {
    let chars: Vec<char> = mine.chars().collect();
    let mut out = Vec::new();
    let mut at = 0;
    let equal = |out: &mut Vec<(String, Option<bool>)>, from: usize, to: usize| {
        if from < to {
            out.push((chars[from..to].iter().collect(), None));
        }
    };
    for edit in diff_text(mine, theirs) {
        match edit {
            TextEdit::Insert { at: pos, text } => {
                equal(&mut out, at, pos);
                out.push((text, Some(true)));
                at = pos;
            }
            TextEdit::Delete { at: pos, len } => {
                equal(&mut out, at, pos);
                out.push((chars[pos..pos + len].iter().collect(), Some(false)));
                at = pos + len;
            }
        }
    }
    equal(&mut out, at, chars.len());
    out
}

/// Draws the sheet centred over `screen` for the selected flag, and records the buttons. Nothing
/// under it takes a click.
pub fn draw(frame: &mut Frame, screen: Rect, state: &AppState, hits: &mut HitMap) {
    hits.push(screen, Target::Inert);
    let Some(flag) = state.needs_review.get(state.conflict_cursor) else {
        return;
    };
    let width = screen.width.saturating_sub(4).min(90);
    let area = Rect::new(
        screen.x + (screen.width - width) / 2,
        screen.y + screen.height.saturating_sub(9) / 2,
        width,
        9.min(screen.height),
    );
    let count = state.needs_review.len();
    let title = format!(
        " Review line {} \u{b7} {} of {count} ",
        flag.line_number,
        state.conflict_cursor + 1
    );
    frame.render_widget(Clear, area);
    frame.render_widget(Block::default().borders(Borders::ALL).title(title), area);
    let inner = area.inner(Margin::new(2, 1));
    frame.render_widget(Paragraph::new(body(flag, count - 1)), inner);
    buttons(frame, inner, hits);
}

/// The sheet's text: the note, mine, theirs, the merged preview and how many flags follow.
fn body(flag: &crate::state::ConflictItem, more: usize) -> Vec<Line<'static>> {
    let dim = Style::new().add_modifier(Modifier::DIM);
    let added = Style::new().add_modifier(Modifier::UNDERLINED | Modifier::BOLD);
    let mut merged = vec![Span::styled("Merged  ", dim)];
    merged.extend(
        merged_runs(&flag.mine, &flag.theirs)
            .into_iter()
            .map(|(text, changed)| match changed {
                Some(true) => Span::styled(text, added),
                Some(false) => Span::styled(text, dim.add_modifier(Modifier::CROSSED_OUT)),
                None => Span::raw(text),
            }),
    );
    let more = if more > 0 {
        format!("{more} more after this: j / k")
    } else {
        String::new()
    };
    vec![
        Line::styled(
            "Two devices edited this line. It is read-only until you pick.",
            dim,
        ),
        Line::default(),
        Line::from(vec![
            Span::styled("Mine    ", dim),
            Span::raw(flag.mine.clone()),
        ]),
        Line::from(vec![
            Span::styled("Theirs  ", dim),
            Span::raw(flag.theirs.clone()),
        ]),
        Line::from(merged),
        Line::styled(more, dim),
    ]
}

/// The choices along the bottom of `inner`, each a click target.
fn buttons(frame: &mut Frame, inner: Rect, hits: &mut HitMap) {
    let y = inner.bottom().saturating_sub(1).max(inner.y);
    let mut x = inner.x;
    for (label, command) in BUTTONS {
        let text = format!(" {label} ");
        let w = u16::try_from(Span::raw(text.as_str()).width()).unwrap_or(0);
        if x + w > inner.right() {
            break;
        }
        let rect = Rect::new(x, y, w, 1);
        let style = Style::new().add_modifier(Modifier::REVERSED);
        frame.render_widget(Paragraph::new(text).style(style), rect);
        hits.push(rect, Target::Command(command));
        x += w + 2;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    #[test]
    fn the_merged_preview_marks_what_each_side_added() {
        let runs = merged_runs("Renew passport +admin", "Renew passport +admin +urgent");
        assert_eq!(
            runs,
            [
                ("Renew passport +admin".to_owned(), None),
                (" +urgent".to_owned(), Some(true))
            ]
        );
        let runs = merged_runs("call mum", "call mom");
        let text: String = runs.iter().map(|(t, _)| t.as_str()).collect();
        assert!(
            text.contains('u') && text.contains('o'),
            "both sides show: {text}"
        );
    }

    #[test]
    fn the_sheet_counts_flags_and_its_buttons_resolve() {
        let mut state = AppState::fixture();
        let mut second = state.needs_review[0].clone();
        second.line_number = 4;
        state.needs_review.push(second);
        let mut terminal =
            Terminal::new(TestBackend::new(80, 14)).unwrap_or_else(|e| panic!("{e}"));
        let mut hits = HitMap::default();
        terminal
            .draw(|f| draw(f, f.area(), &state, &mut hits))
            .unwrap_or_else(|e| panic!("{e}"));
        let rows: Vec<String> = terminal
            .backend()
            .buffer()
            .content()
            .chunks(80)
            .map(|r| r.iter().map(|c| c.symbol()).collect())
            .collect();
        let all = rows.join("\n");
        assert!(all.contains("Review line 2 \u{b7} 1 of 2"), "{all}");
        assert!(all.contains("1 more after this"), "{all}");
        let (y, row) = rows
            .iter()
            .enumerate()
            .find(|(_, r)| r.contains("Keep theirs"))
            .unwrap_or_else(|| panic!("{all}"));
        let x = u16::try_from(row.find("Keep theirs").unwrap_or(0)).unwrap_or(0);
        let y = u16::try_from(y).unwrap_or(0);
        assert_eq!(
            hits.at(x, y),
            Some(Target::Command(Command::ConflictsKeepTheirs))
        );
        assert_eq!(hits.at(0, 0), Some(Target::Inert), "the screen under it");
    }
}
