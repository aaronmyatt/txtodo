//! The header row (task `tui-revamp/tui-shell`, c2 `c2-prompt.html:91-133`), left to right: the
//! live mark, the workspace name (opens the `W` popup), the search field, the Tasks / Universal /
//! Settings tabs and `?`. On a narrow terminal the tabs shrink to their first letter. Every part
//! records its hit target, so a click does what its key does.
//! Ref: <https://docs.rs/ratatui/latest/ratatui/text/struct.Span.html>

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::hit::{HitMap, Target};
use crate::keymap::Command;
use crate::state::AppState;
use crate::state_nav::Screen;

/// Below this width the tabs are letters.
const WIDE: u16 = 80;
/// The longest workspace name shown whole.
const NAME_MAX: usize = 24;
/// The narrowest search field worth drawing.
const SEARCH_MIN: u16 = 8;

/// One clickable piece of the header, in drawing order.
struct Part {
    span: Span<'static>,
    target: Option<Target>,
}

fn part(text: impl Into<String>, style: Style, target: Option<Command>) -> Part {
    Part {
        span: Span::styled(text.into(), style),
        target: target.map(Target::Command),
    }
}

/// Draws the header into `area` (one row) and records its targets.
pub fn draw(frame: &mut Frame, area: Rect, state: &AppState, hits: &mut HitMap) {
    let mut left = crate::ui::mark::spans(state)
        .into_iter()
        .map(|span| Part { span, target: None })
        .collect::<Vec<_>>();
    left.push(part(" ", Style::new(), None));
    left.push(part(
        format!("{} \u{25be}", workspace_name(state)),
        Style::new().add_modifier(Modifier::BOLD),
        Some(Command::NavWorkspaceMenu),
    ));
    left.push(part("  ", Style::new(), None));
    let right = tabs(state.nav.screen, area.width >= WIDE);
    let used: usize = left.iter().chain(&right).map(|p| p.span.width()).sum();
    let room = usize::from(area.width).saturating_sub(used + 2);
    let mut parts = left;
    if room >= usize::from(SEARCH_MIN) {
        parts.push(search_field(state, room));
    } else {
        parts.push(part(" ".repeat(room), Style::new(), None));
    }
    parts.push(part("  ", Style::new(), None));
    parts.extend(right);
    place(frame, area, parts, hits);
}

/// Draws `parts` left to right from `area`'s left edge, recording each target's cells.
fn place(frame: &mut Frame, area: Rect, parts: Vec<Part>, hits: &mut HitMap) {
    let mut x = area.x;
    let mut spans = Vec::with_capacity(parts.len());
    for p in parts {
        let width = u16::try_from(p.span.width()).unwrap_or(u16::MAX);
        let visible = width.min(area.right().saturating_sub(x));
        if let Some(target) = p.target
            && visible > 0
        {
            hits.push(Rect::new(x, area.y, visible, 1), target);
        }
        x = x.saturating_add(width);
        spans.push(p.span);
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

/// The header's name for the open workspace: its label when it has one (the default workspace, a
/// remote mirror), else its folder, cut to [`NAME_MAX`] columns.
pub fn workspace_name(state: &AppState) -> String {
    let name = state.workspace_label.clone().unwrap_or_else(|| {
        std::path::Path::new(&state.shell.root)
            .file_name()
            .map_or_else(
                || "workspace".to_owned(),
                |n| n.to_string_lossy().into_owned(),
            )
    });
    if Span::raw(name.as_str()).width() <= NAME_MAX {
        return name;
    }
    let cut: String = name.chars().take(NAME_MAX - 1).collect();
    format!("{cut}\u{2026}")
}

/// The search field: the query, or a dim placeholder naming the open file, underlined as a field,
/// `width` columns wide. Not clickable until search lands (task `tui-revamp/tui-tasks`).
fn search_field(state: &AppState, width: usize) -> Part {
    let file = std::path::Path::new(&state.path)
        .file_name()
        .map_or_else(|| state.path.clone(), |n| n.to_string_lossy().into_owned());
    let (text, style) = if state.shell.search.is_empty() {
        (
            format!("/ Search {file}"),
            Style::new().add_modifier(Modifier::DIM | Modifier::UNDERLINED),
        )
    } else {
        (
            format!("/ {}", state.shell.search),
            Style::new().add_modifier(Modifier::UNDERLINED),
        )
    };
    let text: String = text.chars().take(width).collect();
    let pad = width.saturating_sub(Span::raw(text.as_str()).width());
    Part {
        span: Span::styled(format!("{text}{}", " ".repeat(pad)), style),
        target: Some(Target::Inert),
    }
}

/// The three tabs, the current one reversed, then `?`.
fn tabs(screen: Screen, wide: bool) -> Vec<Part> {
    let tab = |label: &str, command, on: bool| {
        let text = if wide {
            format!(" {label} ")
        } else {
            format!(" {} ", &label[..1])
        };
        let style = if on {
            Style::new().add_modifier(Modifier::REVERSED)
        } else {
            Style::new()
        };
        part(text, style, Some(command))
    };
    vec![
        tab("Tasks", Command::NavTasks, screen == Screen::Tasks),
        tab(
            "Universal",
            Command::NavUniversal,
            screen == Screen::Universal,
        ),
        tab(
            "Settings",
            Command::NavSettings,
            matches!(screen, Screen::Settings(_)),
        ),
        part(" ", Style::new(), None),
        tab("?", Command::NavHelp, screen == Screen::Help),
    ]
}

#[cfg(test)]
#[path = "header_tests.rs"]
mod tests;
