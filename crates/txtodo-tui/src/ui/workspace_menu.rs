//! The `W` workspace popup (task `tui-revamp/tui-shell`, c2 header dropdown): one row per
//! workspace with its open count, the open one marked, a missing folder said so, and a last
//! "Manage workspaces" row that goes to Settings › Workspaces. It hangs under the header's
//! workspace name; a click outside it closes it.
//! Ref: <https://docs.rs/ratatui/latest/ratatui/widgets/struct.Clear.html>

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState};

use crate::hit::{HitMap, Target};
use crate::keymap::Command;
use crate::state_shell::{MenuItem, WorkspaceMenu};

/// The popup's width, borders included (the c2 dropdown's `w-72`, in cells).
const WIDTH: u16 = 40;

/// Draws the popup over `screen`, hanging from row `top`, and records its rows; everything else
/// on screen closes it when clicked.
pub fn draw(frame: &mut Frame, screen: Rect, top: u16, menu: &WorkspaceMenu, hits: &mut HitMap) {
    hits.push(screen, Target::Command(Command::WorkspaceMenuClose));
    let rows = u16::try_from(menu.items.len() + 1).unwrap_or(u16::MAX);
    let height = (rows + 2).min(screen.bottom().saturating_sub(top));
    let area = Rect::new(screen.x, top, WIDTH.min(screen.width), height);
    let mut items: Vec<ListItem> = menu.items.iter().map(|i| ListItem::new(row(i))).collect();
    items.push(ListItem::new(Line::from(Span::styled(
        "Manage workspaces\u{2026}",
        Style::new().add_modifier(Modifier::ITALIC),
    ))));
    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title(" Workspaces "))
        .highlight_style(Style::new().add_modifier(Modifier::REVERSED));
    let mut list_state = ListState::default().with_selected(Some(menu.cursor));
    frame.render_widget(Clear, area);
    frame.render_stateful_widget(list, area, &mut list_state);
    hits.push(area, Target::Inert);
    let inner = area.inner(ratatui::layout::Margin::new(1, 1));
    let first = list_state.offset();
    for (i, y) in (inner.y..inner.bottom()).enumerate() {
        let index = first + i;
        if index <= menu.items.len() {
            hits.push(
                Rect::new(inner.x, y, inner.width, 1),
                Target::MenuItem(index),
            );
        }
    }
}

/// `• name   N open`: the dot marks the open workspace; a missing folder says so instead.
fn row(item: &MenuItem) -> Line<'static> {
    let mark = if item.current { "\u{2022} " } else { "  " };
    let detail = match item.open {
        _ if item.missing => "missing".to_owned(),
        Some(n) => format!("{n} open"),
        None => "not loaded".to_owned(),
    };
    let name_style = if item.missing {
        Style::new().add_modifier(Modifier::DIM | Modifier::CROSSED_OUT)
    } else if item.current {
        Style::new().add_modifier(Modifier::BOLD)
    } else {
        Style::new()
    };
    Line::from(vec![
        Span::raw(mark),
        Span::styled(item.name.clone(), name_style),
        Span::styled(
            format!("  {detail}"),
            Style::new().add_modifier(Modifier::DIM),
        ),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    #[test]
    fn rows_are_clickable_and_the_rest_of_the_screen_closes_it() {
        let menu = {
            let mut m = WorkspaceMenu::default();
            let item = |name: &str, current, missing| MenuItem {
                name: name.to_owned(),
                current,
                missing,
                open: Some(3),
                ..MenuItem::default()
            };
            m.fill(vec![item("notes", true, false), item("old", false, true)]);
            m
        };
        let mut terminal =
            Terminal::new(TestBackend::new(60, 12)).unwrap_or_else(|e| panic!("{e}"));
        let mut hits = HitMap::default();
        terminal
            .draw(|f| draw(f, f.area(), 1, &menu, &mut hits))
            .unwrap_or_else(|e| panic!("{e}"));
        let text: String = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|c| c.symbol())
            .collect();
        assert!(text.contains("\u{2022} notes  3 open"), "{text}");
        assert!(text.contains("old  missing"), "{text}");
        assert_eq!(hits.at(2, 2), Some(Target::MenuItem(0)));
        assert_eq!(
            hits.at(2, 4),
            Some(Target::MenuItem(2)),
            "Manage workspaces"
        );
        assert_eq!(hits.at(0, 2), Some(Target::Inert), "the border");
        assert_eq!(
            hits.at(50, 8),
            Some(Target::Command(Command::WorkspaceMenuClose))
        );
    }
}
