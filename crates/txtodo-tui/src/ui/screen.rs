//! The top-level frame: composes `list`/`sync`/`conflicts`/`edit` into one `ratatui::Frame`
//! (design §7). Kept separate from `app.rs`'s event loop so the loop's async plumbing and the
//! (synchronous, easy to keep small) rendering code don't share one file.

use std::time::Instant;

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{ListState, Paragraph};

use crate::hit::{HitMap, Target};
use crate::state::{AppState, EditTarget};
use crate::state_nav::{Focus, Overlay, Screen};
use crate::ui::{
    banner, conflict_sheet, detail, footer, header, help, list, offers, prompt_bar, search_panel,
    settings, subbar, sync, toast, universal, workspace_menu,
};

/// Renders one frame: the header, the screen in view, the status line, and whichever overlay (the
/// `W` popup, `edit`, the `r` pane, the `:` line) is active. Returns where the clickable things
/// landed (task `tui-revamp/tui-mouse`); an overlay covers what is under it.
pub fn draw(frame: &mut Frame, state: &AppState) -> HitMap {
    let mut hits = HitMap::default();
    let banners = banner::banners(state);
    let banner_rows = u16::try_from(banners.len()).unwrap_or(u16::MAX);
    let [
        header_area,
        banner_area,
        main_area,
        prompt_area,
        status_area,
    ] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(banner_rows),
        Constraint::Min(1),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .areas(frame.area());
    header::draw(frame, header_area, state, &mut hits);
    banner::draw(frame, banner_area, &banners, &mut hits);
    match state.nav.screen {
        Screen::Tasks => draw_tasks(frame, main_area, state, &mut hits),
        Screen::Universal => universal::draw(frame, main_area, state, &mut hits),
        Screen::Settings(card) => settings::draw(frame, main_area, state, card, &mut hits),
        Screen::Help => help::draw(frame, main_area, state),
    }
    let now = Instant::now();
    footer::draw(frame, status_area, state, now, &mut hits);
    prompt_bar::draw(frame, prompt_area, state, now, &mut hits);
    toast::draw(frame, main_area, &state.shell, now, &mut hits);
    if state.sync_visible {
        sync::draw_popup(frame, main_area, &state.sync, &mut hits);
    }
    if state.nav.focus == Focus::Search {
        let screen = frame.area();
        search_panel::draw(frame, screen, header_area.bottom(), state, &mut hits);
    }
    if state.conflicts_open {
        conflict_sheet::draw(frame, frame.area(), state, &mut hits);
    }
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

/// The Tasks screen: the sub-toolbar, the line list and the overlays that sit on it.
fn draw_tasks(frame: &mut Frame, area: Rect, state: &AppState, hits: &mut HitMap) {
    // The detail panel takes the bottom 55% and the list stays above it, as in c2.
    let panel = if state.detail.is_open() { 55 } else { 0 };
    let [bar_area, list_area, panel_area] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(1),
        Constraint::Percentage(panel),
    ])
    .areas(area);
    subbar::draw(frame, bar_area, state);
    detail::draw(frame, panel_area, state, hits);
    // Start from the last frame's scroll; ratatui moves it only to keep the cursor in view.
    // Ref: https://docs.rs/ratatui/latest/ratatui/widgets/struct.ListState.html
    let mut list_state = ListState::default()
        .with_offset(state.scroll)
        .with_selected(Some(state.cursor));
    frame.render_stateful_widget(list::list_widget(state), list_area, &mut list_state);
    hits.record_list(list_area, list_state.offset(), state.row_count());

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

/// One bold line over the bottom of `area` (the edit and `:` lines); returns where it went.
fn draw_overlay(frame: &mut Frame, area: Rect, content: Line<'static>) -> Rect {
    let y = area.y + area.height.saturating_sub(1);
    let rect = Rect::new(area.x, y, area.width, 1.min(area.height));
    frame.render_widget(
        Paragraph::new(content).style(Style::new().add_modifier(Modifier::BOLD)),
        rect,
    );
    rect
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::AppState;

    /// Draws `state` on a 40x9 test terminal and returns the hit map.
    /// Ref: https://docs.rs/ratatui/latest/ratatui/backend/struct.TestBackend.html
    fn drawn(state: &AppState) -> HitMap {
        let backend = ratatui::backend::TestBackend::new(40, 9);
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
        assert_eq!(
            drawn(&state).at(0, 1),
            Some(Target::Inert),
            "the conflict banner"
        );
        state.needs_review.clear();
        let hits = drawn(&state);
        assert_eq!(hits.at(0, 1), None, "the sub-toolbar");
        assert_eq!(hits.at(0, 2), Some(Target::Row(0)), "under the sub-toolbar");
        assert_eq!(hits.at(0, 6), Some(Target::Row(4)), "the Add-a-line row");
        assert_eq!(
            hits.at(0, 7),
            Some(Target::Command(crate::keymap::Command::PromptFocus)),
            "the prompt bar"
        );
        assert_eq!(
            hits.at(0, 8),
            Some(Target::Command(crate::keymap::Command::SyncOpen)),
            "the footer's status opens the sync popup"
        );
        state.command = Some(String::new());
        assert_eq!(drawn(&state).at(0, 6), Some(Target::Inert), "the : line");
        state.command = None;
        state.nav.overlay = Some(Overlay::WorkspaceMenu);
        assert_eq!(
            drawn(&state).at(39, 6),
            Some(Target::Command(crate::keymap::Command::WorkspaceMenuClose)),
            "the popup's outside closes it"
        );
        state.nav.overlay = None;
        state.nav.screen = Screen::Universal;
        assert_eq!(drawn(&state).at(0, 1), None, "no list on another screen");
    }
}
