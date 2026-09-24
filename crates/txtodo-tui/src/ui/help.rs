//! The Help screen (`?`, task `tui-revamp`): every key the TUI binds, grouped by where it works,
//! read from `specs/client-parity.toml` as built (`manifest::shortcuts`), then the search operators
//! and the prompt bar's chips. Rendered from the manifest so it cannot drift from the keymap.
//! Ref: <https://docs.rs/ratatui/latest/ratatui/widgets/struct.Paragraph.html#method.scroll>

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::manifest::shortcuts;
use crate::state::AppState;

/// The scopes in the order Help lists them, with their headings.
const SCOPES: [(&str, &str); 9] = [
    ("global", "Anywhere"),
    ("list", "Tasks list"),
    ("edit", "Editing a line"),
    ("search", "Search"),
    ("prompt", "Prompt bar"),
    ("detail", "Detail panel"),
    ("universal", "Universal"),
    ("settings", "Settings"),
    ("sheet", "Sheets and popups"),
];

/// The search operators (core `query::matches`).
const OPERATORS: [(&str, &str); 5] = [
    ("word", "lines that contain it, any case; words are AND-ed"),
    ("-word", "lines that do not contain it"),
    ("is:open", "lines not done"),
    ("is:done", "done lines (Universal shows them too)"),
    ("+project @context due:", "any token, as text"),
];

/// Every line of the screen, headings bold, keys in a column.
pub fn lines() -> Vec<Line<'static>> {
    let bold = Style::new().add_modifier(Modifier::BOLD);
    let dim = Style::new().add_modifier(Modifier::DIM);
    let all = shortcuts();
    let mut out = vec![Line::styled(" Help: keys, search, the prompt bar", bold)];
    for (scope, heading) in SCOPES {
        let rows: Vec<_> = all.iter().filter(|s| s.scope == scope).collect();
        if rows.is_empty() {
            continue;
        }
        out.push(Line::default());
        out.push(Line::styled(format!(" {heading}"), bold));
        for s in rows {
            let keys = if s.keys.is_empty() {
                format!(":{}", s.id)
            } else {
                s.keys.join(" / ")
            };
            out.push(Line::from(vec![
                Span::raw(format!("   {keys:<24}")),
                Span::styled(s.title.clone(), dim),
            ]));
        }
    }
    out.push(Line::default());
    out.push(Line::styled(" Search operators", bold));
    for (op, what) in OPERATORS {
        out.push(Line::from(vec![
            Span::raw(format!("   {op:<24}")),
            Span::styled(what, dim),
        ]));
    }
    out.push(Line::default());
    out.push(Line::styled(
        " Prompt bar chips (Alt and the key, or click)",
        bold,
    ));
    for (label, key, _) in crate::prompt::CHIPS {
        out.push(Line::from(vec![
            Span::raw(format!("   Alt-{key:<20}")),
            Span::styled(label, dim),
        ]));
    }
    out
}

/// Draws the screen into `area`, scrolled by `state.shell.help_scroll` lines.
pub fn draw(frame: &mut Frame, area: Rect, state: &AppState) {
    let scroll = (state.shell.help_scroll, 0);
    frame.render_widget(Paragraph::new(lines()).scroll(scroll), area);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn help_lists_keys_by_scope_then_operators_and_chips() {
        let text: Vec<String> = lines().iter().map(ToString::to_string).collect();
        let at = |needle: &str| {
            text.iter()
                .position(|l| l.contains(needle))
                .unwrap_or_else(|| panic!("{needle} in {text:#?}"))
        };
        assert!(at(" Anywhere") < at(" Tasks list"));
        assert!(text[at("Ctrl-c")].contains("Quit"));
        assert!(at(" Search operators") > at(" Settings"));
        assert!(text[at("is:done")].contains("done lines"));
        assert!(text[at("Alt-a")].contains("(A)"));
    }
}
