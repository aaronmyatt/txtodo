//! The review sheet for a duplicate group (ADR 0032, task sync-drift duplicate-flags): two or more
//! lines that read the same, which copies there are, and the two choices. `conflict_sheet.rs`
//! draws this instead of a flag when the cursor is past the flags. Unlike a flag, a group never
//! makes the list read-only: editing one copy so they differ also clears it, and the sheet says so.
//! Ref: <https://docs.rs/ratatui/latest/ratatui/widgets/struct.Clear.html>

use ratatui::Frame;
use ratatui::layout::{Margin, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

use crate::hit::{HitMap, Target};
use crate::keymap::Command;
use crate::state::{AppState, DuplicateGroup};

/// The buttons, left to right: label and command.
const BUTTONS: [(&str, Command); 3] = [
    ("n Keep newest", Command::ConflictsKeepNewest),
    ("o Keep oldest", Command::ConflictsKeepOldest),
    ("Esc Close", Command::ConflictsClose),
];

/// Draws the sheet for `group` (the item under `state.conflict_cursor`) centred in `screen`, and
/// records its buttons.
pub fn draw(
    frame: &mut Frame,
    screen: Rect,
    state: &AppState,
    group: &DuplicateGroup,
    hits: &mut HitMap,
) {
    let width = screen.width.saturating_sub(4).min(90);
    let area = Rect::new(
        screen.x + (screen.width - width) / 2,
        screen.y + screen.height.saturating_sub(8) / 2,
        width,
        8.min(screen.height),
    );
    let count = state.review_len();
    let title = format!(
        " Same line \u{b7} {} of {count} ",
        state.conflict_cursor + 1
    );
    frame.render_widget(Clear, area);
    frame.render_widget(Block::default().borders(Borders::ALL).title(title), area);
    let inner = area.inner(Margin::new(2, 1));
    let more = count.saturating_sub(state.conflict_cursor + 1);
    frame.render_widget(Paragraph::new(body(state, group, more)), inner);
    buttons(frame, inner, hits);
}

/// The note, the line, where its copies are, and how many items follow.
fn body(state: &AppState, group: &DuplicateGroup, more: usize) -> Vec<Line<'static>> {
    let dim = Style::new().add_modifier(Modifier::DIM);
    let text = group
        .copies
        .first()
        .and_then(|c| state.lines.get((c.line_number as usize).checked_sub(1)?))
        .map(|l| l.raw.clone())
        .unwrap_or_default();
    let newest = group.copies.len().saturating_sub(1);
    let places: Vec<String> = group
        .copies
        .iter()
        .enumerate()
        .map(|(i, c)| match i {
            0 => format!("line {} (oldest)", c.line_number),
            i if i == newest => format!("line {} (newest)", c.line_number),
            _ => format!("line {}", c.line_number),
        })
        .collect();
    let more = if more > 0 {
        format!("{more} more after this: j / k")
    } else {
        String::new()
    };
    vec![
        Line::styled(
            "These lines read the same. Keep one, or edit one so they differ.",
            dim,
        ),
        Line::default(),
        Line::from(vec![Span::styled("Line    ", dim), Span::raw(text)]),
        Line::from(vec![
            Span::styled("Copies  ", dim),
            Span::raw(places.join(", ")),
        ]),
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
    use crate::state::DuplicateCopy;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    #[test]
    fn the_sheet_names_the_line_its_copies_and_the_two_choices() {
        let mut state = AppState::from_document("todo.txt", "buy milk\nwalk dog\nbuy milk\n");
        let copy = |id: &str, line| DuplicateCopy {
            task_id: id.to_owned(),
            line_number: line,
        };
        let group = DuplicateGroup {
            copies: vec![
                copy("01J9K3H5Z7Q8X2M4N6P8R0T2V1", 1),
                copy("01J9K3H5Z7Q8X2M4N6P8R0T2V2", 3),
            ],
        };
        state.duplicates.push(group.clone());
        let mut terminal =
            Terminal::new(TestBackend::new(80, 12)).unwrap_or_else(|e| panic!("{e}"));
        let mut hits = HitMap::default();
        terminal
            .draw(|f| draw(f, f.area(), &state, &group, &mut hits))
            .unwrap_or_else(|e| panic!("{e}"));
        let rows: Vec<String> = terminal
            .backend()
            .buffer()
            .content()
            .chunks(80)
            .map(|r| r.iter().map(|c| c.symbol()).collect())
            .collect();
        let all = rows.join("\n");
        assert!(all.contains("Same line \u{b7} 1 of 1"), "{all}");
        assert!(all.contains("buy milk"), "{all}");
        assert!(all.contains("line 1 (oldest), line 3 (newest)"), "{all}");
        let (y, row) = rows
            .iter()
            .enumerate()
            .find(|(_, r)| r.contains("Keep oldest"))
            .unwrap_or_else(|| panic!("{all}"));
        let x = u16::try_from(row.find("Keep oldest").unwrap_or(0)).unwrap_or(0);
        let y = u16::try_from(y).unwrap_or(0);
        assert_eq!(
            hits.at(x, y),
            Some(Target::Command(Command::ConflictsKeepOldest))
        );
    }
}
