//! The top-level frame: composes `list`/`sync`/`conflicts`/`edit` into one `ratatui::Frame`
//! (design §7). Kept separate from `app.rs`'s event loop so the loop's async plumbing and the
//! (synchronous, easy to keep small) rendering code don't share one file.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph};

use crate::hit::{HitMap, Target};
use crate::state::{AppState, EditTarget};
use crate::state_nav::{Overlay, Screen};
use crate::ui::{header, list, offers, sync, workspace_menu};

/// Renders one frame: the header, the screen in view, the status line, and whichever overlay (the
/// `W` popup, `edit`, the `r` pane, the `:` line) is active. Returns where the clickable things
/// landed (task `tui-revamp/tui-mouse`); an overlay covers what is under it.
pub fn draw(frame: &mut Frame, state: &AppState) -> HitMap {
    let mut hits = HitMap::default();
    let [header_area, main_area, status_area] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(1),
        Constraint::Length(1),
    ])
    .areas(frame.area());
    header::draw(frame, header_area, state, &mut hits);
    match state.nav.screen {
        Screen::Tasks => draw_tasks(frame, main_area, state, &mut hits),
        other => draw_placeholder(frame, main_area, other),
    }
    frame.render_widget(
        Paragraph::new(status_line(state, status_area.width)),
        status_area,
    );
    if state.nav.overlay == Some(Overlay::WorkspaceMenu) {
        let screen = frame.area();
        workspace_menu::draw(
            frame,
            screen,
            header_area.bottom(),
            &state.shell.menu,
            &mut hits,
        );
    }
    if let Some(cmd) = &state.command {
        let line = Line::from(format!(":{cmd}"));
        hits.push(draw_overlay(frame, main_area, line), Target::Inert);
    }
    hits
}

/// A screen that is not built yet says so, and where its plan is.
fn draw_placeholder(frame: &mut Frame, area: Rect, screen: Screen) {
    let (name, task) = match screen {
        Screen::Universal => ("Universal", "tui-universal"),
        Screen::Settings(_) => ("Settings", "tui-settings"),
        Screen::Help | Screen::Tasks => ("Help", "tui-revamp"),
    };
    let text =
        format!("  {name} is not built yet (tasks/tui-revamp/{task}). g t goes back to Tasks.");
    frame.render_widget(
        Paragraph::new(text).style(Style::new().add_modifier(Modifier::DIM)),
        area,
    );
}

/// The Tasks screen: the line list and the overlays that sit on it.
fn draw_tasks(frame: &mut Frame, list_area: Rect, state: &AppState, hits: &mut HitMap) {
    // Start from the last frame's scroll; ratatui moves it only to keep the cursor in view.
    // Ref: https://docs.rs/ratatui/latest/ratatui/widgets/struct.ListState.html
    let mut list_state = ListState::default()
        .with_offset(state.scroll)
        .with_selected(Some(state.cursor));
    frame.render_stateful_widget(list::list_widget(state), list_area, &mut list_state);
    hits.record_list(list_area, list_state.offset(), state.row_count());

    if state.sync_visible {
        hits.push(
            draw_overlay(frame, list_area, sync::render(&state.sync)),
            Target::Inert,
        );
    }
    if state.conflicts_open {
        draw_conflicts(frame, list_area, state);
        hits.push(list_area, Target::Inert);
    }
    if state.offers.open {
        offers::draw(frame, list_area, state);
        hits.push(list_area, Target::Inert);
    }
    if let Some(draft) = &state.editing {
        let line = Line::from(format!("{}> {}", edit_label(&draft.target), draft.buffer));
        hits.push(draw_overlay(frame, list_area, line), Target::Inert);
    }
}

fn edit_label(target: &EditTarget) -> &'static str {
    match target {
        EditTarget::Existing { .. } => "edit ",
        EditTarget::NewLine => "add ",
    }
}

/// The path, the workspace, pending offers, a refusal and the skill hint on the left; this
/// build's version and date on the right, dim. The version is the first thing to go: it is shown only when the whole line still
/// fits in `width` columns with a gap, so a narrow terminal loses nothing it needs.
/// `Line::width`: https://docs.rs/ratatui/latest/ratatui/text/struct.Line.html#method.width
fn status_line(state: &AppState, width: u16) -> Line<'static> {
    let hint = if state.skill_hint {
        " \u{b7} no agent playbook installed; run `txtodo skill install`"
    } else {
        ""
    };
    let workspace = state
        .workspace_label
        .as_deref()
        .map_or_else(String::new, |label| format!(" \u{b7} {label}"));
    let pending = match state.offers.items.len() {
        0 if !state.offers.problem.is_empty() => " \u{b7} offers blocked: o".to_owned(),
        0 => String::new(),
        n => format!(" \u{b7} {n} workspace offer(s): o"),
    };
    let error = state
        .last_error
        .as_deref()
        .map_or_else(String::new, |e| format!(" \u{b7} refused: {e}"));
    let left = format!(" {}{workspace}{pending}{error}{hint}", state.path);
    let version = format!("{} ", crate::buildinfo::UI_LABEL);
    let used = Line::from(left.as_str()).width() + Line::from(version.as_str()).width();
    let Some(gap) = usize::from(width).checked_sub(used).filter(|gap| *gap >= 2) else {
        return Line::from(left);
    };
    Line::from(vec![
        Span::raw(left),
        Span::raw(" ".repeat(gap)),
        Span::styled(version, Style::new().add_modifier(Modifier::DIM)),
    ])
}

/// A one-line strip anchored to the bottom of `area` — good enough for the sync indicator and
/// the edit/command line; a real popup (with a border) is a rendering-polish follow-up, not
/// required by any acceptance criterion.
/// One bold line over the bottom of `area`; returns where it went.
fn draw_overlay(frame: &mut Frame, area: Rect, content: Line<'static>) -> Rect {
    let y = area.y + area.height.saturating_sub(1);
    let rect = Rect::new(area.x, y, area.width, 1.min(area.height));
    frame.render_widget(
        Paragraph::new(content).style(Style::new().add_modifier(Modifier::BOLD)),
        rect,
    );
    rect
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
        let text = status_line(&state, 120).to_string();
        assert!(!text.contains("skill install"));
        assert!(text.contains(&state.path));
    }

    #[test]
    fn status_line_shows_the_version_dim_at_the_right_edge_when_it_fits() {
        let state = AppState::fixture();
        let line = status_line(&state, 120);
        assert_eq!(line.width(), 120, "padded out to the right edge");
        let last = line.spans.last().unwrap_or_else(|| panic!("spans"));
        assert_eq!(last.content.trim_end(), crate::buildinfo::UI_LABEL);
        assert!(last.style.add_modifier.contains(Modifier::DIM));
    }

    #[test]
    fn a_narrow_terminal_drops_the_version_first() {
        let state = AppState::fixture();
        let narrow = status_line(&state, 30).to_string();
        assert!(!narrow.contains(crate::buildinfo::UI_LABEL));
        assert!(narrow.contains(&state.path), "the rest stays: {narrow}");
    }

    #[test]
    fn status_line_names_the_default_workspace() {
        let mut state = AppState::fixture();
        assert!(
            !status_line(&state, 120)
                .to_string()
                .contains("default workspace")
        );
        state.workspace_label = Some("default workspace".to_owned());
        assert!(
            status_line(&state, 120)
                .to_string()
                .contains("todo.txt \u{b7} default workspace")
        );
    }

    #[test]
    fn status_line_shows_the_daemons_last_refusal() {
        let mut state = AppState::fixture();
        state.last_error = Some("line 3 is blank".to_owned());
        assert!(
            status_line(&state, 120)
                .to_string()
                .contains("refused: line 3 is blank")
        );
        state.last_error = None;
        assert!(!status_line(&state, 120).to_string().contains("refused"));
    }

    /// Draws `state` on a 40x7 test terminal and returns the hit map.
    /// Ref: https://docs.rs/ratatui/latest/ratatui/backend/struct.TestBackend.html
    fn drawn(state: &AppState) -> HitMap {
        let backend = ratatui::backend::TestBackend::new(40, 7);
        let mut terminal = ratatui::Terminal::new(backend).unwrap_or_else(|e| panic!("{e}"));
        let mut hits = HitMap::default();
        terminal
            .draw(|f| hits = draw(f, state))
            .unwrap_or_else(|e| panic!("{e}"));
        hits
    }

    #[test]
    fn a_frame_maps_rows_to_cells_and_an_overlay_covers_them() {
        let mut state = AppState::fixture();
        let hits = drawn(&state);
        assert_eq!(hits.at(0, 1), Some(Target::Row(0)), "under the header");
        assert_eq!(hits.at(0, 5), Some(Target::Row(4)), "the Add-a-line row");
        assert_eq!(hits.at(0, 6), None, "the status line");
        state.command = Some(String::new());
        assert_eq!(drawn(&state).at(0, 5), Some(Target::Inert), "the : line");
        state.command = None;
        state.nav.overlay = Some(Overlay::WorkspaceMenu);
        assert_eq!(
            drawn(&state).at(39, 5),
            Some(Target::Command(crate::keymap::Command::WorkspaceMenuClose)),
            "the popup's outside closes it"
        );
        state.nav.overlay = None;
        state.nav.screen = Screen::Universal;
        assert_eq!(drawn(&state).at(0, 1), None, "no list on another screen");
    }

    #[test]
    fn status_line_shows_hint_when_needed() {
        let mut state = AppState::fixture();
        state.skill_hint = true;
        let text = status_line(&state, 120).to_string();
        assert!(text.contains("no agent playbook installed; run `txtodo skill install`"));
    }
}
