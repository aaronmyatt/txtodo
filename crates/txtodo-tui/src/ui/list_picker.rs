//! The `m`/`c` list picker (`list_picker.rs`): the workspace's other lists, one row each, the
//! selected one reversed, centred over the screen. A click anywhere closes it; the rows are picked
//! with `j`/`k` and Enter.
//! Ref: <https://docs.rs/ratatui/latest/ratatui/widgets/struct.Clear.html>

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState};

use crate::hit::{HitMap, Target};
use crate::keymap::Command;
use crate::list_picker::{ListPicker, PickMode};

/// The popup's width, borders included.
const WIDTH: u16 = 48;

/// Draws the picker centred in `screen`; everything on screen closes it when clicked.
pub fn draw(frame: &mut Frame, screen: Rect, picker: &ListPicker, hits: &mut HitMap) {
    hits.push(screen, Target::Command(Command::ListPickerClose));
    let rows = u16::try_from(picker.lists.len()).unwrap_or(u16::MAX);
    let (width, height) = (WIDTH.min(screen.width), (rows + 2).min(screen.height));
    let area = Rect::new(
        screen.x + (screen.width - width) / 2,
        screen.y + (screen.height - height) / 2,
        width,
        height,
    );
    let title = match picker.mode {
        Some(PickMode::Copy) => " Copy to ",
        _ => " Move to ",
    };
    let items: Vec<ListItem> = picker
        .lists
        .iter()
        .map(|p| ListItem::new(p.clone()))
        .collect();
    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title(title))
        .highlight_style(Style::new().add_modifier(Modifier::REVERSED));
    let mut state = ListState::default().with_selected(Some(picker.cursor));
    frame.render_widget(Clear, area);
    frame.render_stateful_widget(list, area, &mut state);
    hits.push(area, Target::Inert);
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    #[test]
    fn it_names_what_it_does_and_lists_every_target() {
        let picker = ListPicker {
            mode: Some(PickMode::Copy),
            lists: vec!["todo.txt".to_owned(), "tasks/trip/todo.txt".to_owned()],
            cursor: 1,
        };
        let mut terminal =
            Terminal::new(TestBackend::new(60, 10)).unwrap_or_else(|e| panic!("{e}"));
        let mut hits = HitMap::default();
        terminal
            .draw(|f| draw(f, f.area(), &picker, &mut hits))
            .unwrap_or_else(|e| panic!("{e}"));
        let text: String = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|c| c.symbol())
            .collect();
        assert!(text.contains("Copy to"), "{text}");
        assert!(text.contains("tasks/trip/todo.txt"), "{text}");
        assert_eq!(
            hits.at(0, 0),
            Some(Target::Command(Command::ListPickerClose))
        );
    }
}
