//! The top-level frame: composes `list`/`sync`/`conflicts`/`edit` into one `ratatui::Frame`
//! (design §7). Kept separate from `app.rs`'s event loop so the loop's async plumbing and the
//! (synchronous, easy to keep small) rendering code don't share one file.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph};

use crate::state::{AppState, EditTarget};
use crate::ui::{list, sync};

/// Renders one frame: the line list, the status/sync line, and whichever overlay (`edit`/`r`
/// pane/`:` command line) is active.
pub fn draw(frame: &mut Frame, state: &AppState) {
    let [list_area, status_area] =
        Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(frame.area());

    let mut list_state = ListState::default().with_selected(Some(state.cursor));
    frame.render_stateful_widget(list::list_widget(state), list_area, &mut list_state);

    frame.render_widget(Paragraph::new(status_line(state)), status_area);

    if state.sync_visible {
        draw_overlay(frame, list_area, sync::render(&state.sync));
    }
    if state.conflicts_open {
        draw_conflicts(frame, list_area, state);
    }
    if let Some(draft) = &state.editing {
        draw_overlay(
            frame,
            list_area,
            Line::from(format!("{}> {}", edit_label(&draft.target), draft.buffer)),
        );
    }
    if let Some(cmd) = &state.command {
        draw_overlay(frame, list_area, Line::from(format!(":{cmd}")));
    }
}

fn edit_label(target: &EditTarget) -> &'static str {
    match target {
        EditTarget::Existing { .. } => "edit ",
        EditTarget::NewLine => "add ",
    }
}

fn status_line(state: &AppState) -> Line<'static> {
    let id = if state.show_id { "id:on" } else { "id:off" };
    let hint = if state.skill_hint {
        " \u{b7} no agent playbook installed; run `txtodo skill install`"
    } else {
        ""
    };
    Line::from(format!(" {} \u{b7} {id}{hint}", state.path))
}

/// A one-line strip anchored to the bottom of `area` — good enough for the sync indicator and
/// the edit/command line; a real popup (with a border) is a rendering-polish follow-up, not
/// required by any acceptance criterion.
fn draw_overlay(frame: &mut Frame, area: Rect, content: Line<'static>) {
    let y = area.y + area.height.saturating_sub(1);
    let rect = Rect::new(area.x, y, area.width, 1.min(area.height));
    frame.render_widget(
        Paragraph::new(content).style(Style::new().add_modifier(Modifier::BOLD)),
        rect,
    );
}

fn draw_conflicts(frame: &mut Frame, area: Rect, state: &AppState) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title("conflicts: m=mine t=theirs M=merged");
    let items: Vec<ListItem> = state
        .needs_review
        .iter()
        .map(|f| {
            ListItem::new(format!(
                "line {}: mine={:?} theirs={:?}",
                f.line_number, f.mine, f.theirs
            ))
        })
        .collect();
    let mut list_state = ListState::default().with_selected(Some(state.conflict_cursor));
    let list = List::new(items)
        .block(block)
        .highlight_style(Style::new().fg(Color::Black).bg(Color::Yellow));
    frame.render_stateful_widget(list, area, &mut list_state);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::AppState;

    #[test]
    fn status_line_omits_hint_by_default() {
        let state = AppState::fixture();
        let text = status_line(&state).to_string();
        assert!(!text.contains("skill install"));
        assert!(text.contains("id:off"));
    }

    #[test]
    fn status_line_shows_hint_when_needed() {
        let mut state = AppState::fixture();
        state.skill_hint = true;
        let text = status_line(&state).to_string();
        assert!(text.contains("no agent playbook installed; run `txtodo skill install`"));
    }
}
