//! The banners under the header (task `tui-revamp/tui-shell`, c2 `c2-prompt.html:73-89`), one row
//! each, most urgent first: the daemon is not answering (Retry), lines need review (Review), an
//! edit was not saved (Copy edit), the daemon is another build, no agent playbook is installed.
//! Each button is a click target running the command its `:` id names.
//! Ref: <https://docs.rs/ratatui/latest/ratatui/widgets/struct.Paragraph.html>

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::hit::{HitMap, Target};
use crate::keymap::Command;
use crate::state::AppState;
use crate::state_shell::Link;

/// One banner row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Banner {
    /// The bold part.
    pub label: String,
    /// The explanation after it, dim.
    pub detail: String,
    /// Buttons at the right edge: their text and the command they run.
    pub buttons: Vec<(&'static str, Command)>,
    /// Drawn reversed, like the c2 daemon banner.
    pub loud: bool,
}

fn banner(label: impl Into<String>, detail: impl Into<String>) -> Banner {
    Banner {
        label: label.into(),
        detail: detail.into(),
        buttons: Vec::new(),
        loud: false,
    }
}

/// The banners `state` calls for, in drawing order.
pub fn banners(state: &AppState) -> Vec<Banner> {
    let mut out = Vec::new();
    if state.shell.link != Link::Up {
        let label = if state.shell.link == Link::Connecting {
            "daemon: connecting"
        } else {
            "daemon: not answering"
        };
        let mut b = banner(
            label,
            "txtodod isn't answering yet. Edits are not saved until it is.",
        );
        b.buttons.push(("Retry", Command::AppRetryDaemon));
        b.loud = true;
        out.push(b);
    }
    let flagged = state.needs_review.len();
    if flagged > 0 && !state.shell.conflict_banner_hidden {
        let lines = if flagged == 1 {
            "line needs"
        } else {
            "lines need"
        };
        let mut b = banner(
            format!("{flagged} {lines} review"),
            "Two devices edited the same line. It is read-only until you pick.",
        );
        b.buttons.push(("Review", Command::ConflictsOpen));
        b.buttons.push(("\u{d7}", Command::ConflictsDismissBanner));
        out.push(b);
    }
    if let Some(refused) = &state.shell.refused {
        let mut b = banner(
            format!("Your edit to {} was not saved", state.path),
            refused.error.clone(),
        );
        b.buttons.push(("Copy edit", Command::AppCopyRefusedEdit));
        out.push(b);
    }
    if let Some((version, date)) = &state.shell.daemon_build {
        out.push(banner(
            format!("txtodod is v{version} \u{b7} {date}"),
            format!(
                "this TUI is {}. Run `txtodo daemon install`, then `txtodo daemon start`.",
                crate::buildinfo::UI_LABEL
            ),
        ));
    }
    if state.skill_hint {
        let mut b = banner(
            "No agent playbook installed",
            "run `txtodo skill install` in a terminal",
        );
        b.buttons.push(("\u{d7}", Command::AppDismissSkillHint));
        out.push(b);
    }
    out
}

/// Draws `banners` one per row from the top of `area`, buttons flush right, and records the
/// buttons' cells.
pub fn draw(frame: &mut Frame, area: Rect, banners: &[Banner], hits: &mut HitMap) {
    for (b, y) in banners.iter().zip(area.y..area.bottom()) {
        let row = Rect::new(area.x, y, area.width, 1);
        hits.push(row, Target::Inert);
        let base = if b.loud {
            Style::new().add_modifier(Modifier::REVERSED)
        } else {
            Style::new()
        };
        let buttons: Vec<String> = b.buttons.iter().map(|(t, _)| format!(" {t} ")).collect();
        let buttons_width: u16 = buttons
            .iter()
            .map(|t| u16::try_from(Span::raw(t.as_str()).width()).unwrap_or(0) + 1)
            .sum();
        let text_width = usize::from(area.width.saturating_sub(buttons_width));
        let text = format!(" {}  {}", b.label, b.detail);
        let text: String = text.chars().take(text_width).collect();
        let pad = text_width.saturating_sub(Span::raw(text.as_str()).width());
        let label_len = (b.label.chars().count() + 1).min(text.chars().count());
        let (label, detail): (String, String) = (
            text.chars().take(label_len).collect(),
            text.chars().skip(label_len).collect(),
        );
        let mut spans = vec![
            Span::styled(label, base.add_modifier(Modifier::BOLD)),
            Span::styled(detail, base.add_modifier(Modifier::DIM)),
            Span::styled(" ".repeat(pad), base),
        ];
        let mut x = row.x + u16::try_from(text_width).unwrap_or(0);
        for ((_, command), text) in b.buttons.iter().zip(buttons) {
            let width = u16::try_from(Span::raw(text.as_str()).width()).unwrap_or(0);
            hits.push(Rect::new(x, y, width, 1), Target::Command(*command));
            // A button is the banner's colours swapped: reversed on a plain banner, plain on a
            // reversed one.
            let button = if b.loud {
                Style::new()
            } else {
                Style::new().add_modifier(Modifier::REVERSED)
            };
            spans.push(Span::styled(text, button));
            spans.push(Span::styled(" ", base));
            x += width + 1;
        }
        frame.render_widget(Paragraph::new(Line::from(spans)), row);
    }
}

#[cfg(test)]
#[path = "banner_tests.rs"]
mod tests;
