//! The status footer (task `tui-revamp/tui-shell`, c2 `c2-prompt.html:162-168`), one row: the save
//! and sync state (click or `s` for the sync popup), the cursor's line, messages (pending offers,
//! the last refusal), then this screen's key hints and this build's version, right-aligned. On a
//! narrow terminal the version goes first, then the hints. Hint keys come from `keymap::BINDINGS`,
//! which `tests/parity.rs` holds to the manifest.
//! Ref: <https://docs.rs/ratatui/latest/ratatui/text/struct.Line.html#method.width>

use std::time::{Duration, Instant};

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::hit::{HitMap, Target};
use crate::keymap::Command;
use crate::state::AppState;
use crate::state_nav::Screen;

/// How long "saved" shows after an edit lands.
pub const SAVED_FOR: Duration = Duration::from_secs(2);

/// The save and sync state, as the c2 footer names them.
pub fn status(state: &AppState, now: Instant) -> String {
    if state.editing.is_some() {
        return "\u{25cf} unsaved".to_owned();
    }
    if state
        .shell
        .saved_at
        .is_some_and(|at| now.duration_since(at) < SAVED_FOR)
    {
        return "\u{2713} saved".to_owned();
    }
    match state.sync.pending_ops {
        0 => "\u{25cf} synced".to_owned(),
        n => format!("\u{25cf} syncing {n}"),
    }
}

/// `Ln N` for the selected line on the Tasks screen; `Ln +` on the Add-a-line row.
fn caret(state: &AppState) -> Option<String> {
    if state.nav.screen != Screen::Tasks {
        return None;
    }
    Some(match state.selected_line() {
        Some(line) => format!("Ln {}", line.line_number),
        None => "Ln +".to_owned(),
    })
}

/// This screen's hints: a command and the word for it.
fn hints(screen: Screen) -> &'static [(Command, &'static str)] {
    match screen {
        Screen::Tasks => &[
            (Command::ListDown, "down"),
            (Command::ListToggleComplete, "done"),
            (Command::ListEditEnd, "edit"),
            (Command::PaletteOpen, "command"),
            (Command::NavHelp, "help"),
        ],
        Screen::Universal | Screen::Settings(_) | Screen::Help => {
            &[(Command::NavTasks, "tasks"), (Command::NavHelp, "help")]
        }
    }
}

/// `j down  Space done  ...`: each hint's first key and its word.
fn hint_text(screen: Screen) -> String {
    hints(screen)
        .iter()
        .filter_map(|(command, word)| command.keys().first().map(|k| format!("{k} {word}")))
        .collect::<Vec<_>>()
        .join("  ")
}

/// Draws the footer into `area` (one row) and records the status as the sync popup's button.
pub fn draw(frame: &mut Frame, area: Rect, state: &AppState, now: Instant, hits: &mut HitMap) {
    let status = format!(" {} ", status(state, now));
    let status_width = Span::raw(status.as_str()).width();
    hits.push(
        Rect::new(area.x, area.y, u16::try_from(status_width).unwrap_or(0), 1),
        Target::Command(Command::SyncOpen),
    );
    let mut left = String::new();
    if let Some(caret) = caret(state) {
        left.push_str(&format!(" {caret}"));
    }
    match state.offers.items.len() {
        0 if !state.offers.problem.is_empty() => left.push_str(" \u{b7} offers blocked: o"),
        0 => {}
        n => left.push_str(&format!(" \u{b7} {n} workspace offer(s): o")),
    }
    if let Some(e) = &state.last_error {
        left.push_str(&format!(" \u{b7} refused: {e}"));
    }
    let right = right_side(
        state,
        area.width,
        status_width + Span::raw(left.as_str()).width(),
    );
    let gap = usize::from(area.width)
        .saturating_sub(status_width + Span::raw(left.as_str()).width() + right.width());
    let dim = Style::new().add_modifier(Modifier::DIM);
    let mut spans = vec![
        Span::styled(status, Style::new().add_modifier(Modifier::BOLD)),
        Span::raw(left),
        Span::raw(" ".repeat(gap)),
    ];
    spans.extend(right.spans.into_iter().map(|s| s.patch_style(dim)));
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

/// The hints and the version, whichever fit beside `used` columns with a gap of two.
fn right_side(state: &AppState, width: u16, used: usize) -> Line<'static> {
    let hints = format!("{}  ", hint_text(state.nav.screen));
    let version = format!("{} ", crate::buildinfo::UI_LABEL);
    let room = usize::from(width).saturating_sub(used + 2);
    let fits = |s: &str| Span::raw(s).width() <= room;
    let both = format!("{hints}{version}");
    if fits(&both) {
        Line::from(both)
    } else if fits(&hints) {
        Line::from(hints)
    } else {
        Line::default()
    }
}

#[cfg(test)]
#[path = "footer_tests.rs"]
mod tests;
