//! The Settings screen (task `tui-revamp/tui-settings`, c2 `c2/settings.js`): a card nav on the
//! left (the ones the `/` filter keeps), the card in view on the right as rows of label and value.
//! The selected row is reversed; a field row shows what is being typed; a row that cannot exist in
//! a terminal is dim; a pending removal asks for a second `d`. Cards and rows are click targets.
//! Ref: <https://docs.rs/ratatui/latest/ratatui/layout/struct.Layout.html>

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{List, ListItem, ListState, Paragraph};

use crate::commands_settings::visible_cards;
use crate::hit::{HitMap, Target};
use crate::settings_rows::{SRow, rows};
use crate::state::AppState;
use crate::state_nav::SettingsCard;

/// The nav column's width.
const NAV: u16 = 16;
/// The label column's width.
const LABEL: usize = 24;

/// Draws the screen into `area` with `card` in view, and records its targets.
pub fn draw(
    frame: &mut Frame,
    area: Rect,
    state: &AppState,
    card: SettingsCard,
    hits: &mut HitMap,
) {
    let [nav, body] = Layout::horizontal([Constraint::Length(NAV), Constraint::Min(1)]).areas(area);
    draw_nav(frame, nav, state, card, hits);
    let [top, list] = Layout::vertical([Constraint::Length(2), Constraint::Min(1)]).areas(body);
    let settings = &state.settings;
    let filter = if settings.filtering {
        format!("/ {}\u{258f}", settings.filter)
    } else if settings.filter.is_empty() {
        "/ filters the cards".to_owned()
    } else {
        format!("/ {}", settings.filter)
    };
    let heading = Line::from(vec![
        Span::styled(
            format!(" {} ", card.title()),
            Style::new().add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("  {filter}"),
            Style::new().add_modifier(Modifier::DIM),
        ),
    ]);
    frame.render_widget(Paragraph::new(heading), top);
    draw_rows(frame, list, state, &rows(card, state), hits);
}

/// The card nav: every card the filter keeps, the one in view reversed.
fn draw_nav(
    frame: &mut Frame,
    area: Rect,
    state: &AppState,
    card: SettingsCard,
    hits: &mut HitMap,
) {
    let cards = visible_cards(state);
    let items: Vec<ListItem> = cards
        .iter()
        .map(|c| ListItem::new(format!(" {} ", c.title())))
        .collect();
    let at = cards.iter().position(|c| *c == card);
    let mut list_state = ListState::default().with_selected(at);
    let list = List::new(items).highlight_style(Style::new().add_modifier(Modifier::REVERSED));
    frame.render_stateful_widget(list, area, &mut list_state);
    for (c, y) in cards.iter().zip(area.y..area.bottom()) {
        let i = SettingsCard::ALL.iter().position(|x| x == c).unwrap_or(0);
        hits.push(Rect::new(area.x, y, area.width, 1), Target::SettingsCard(i));
    }
}

/// The card's rows.
fn draw_rows(frame: &mut Frame, area: Rect, state: &AppState, rows: &[SRow], hits: &mut HitMap) {
    let settings = &state.settings;
    let items: Vec<ListItem> = rows
        .iter()
        .enumerate()
        .map(|(i, row)| ListItem::new(row_line(row, i == settings.row, state)))
        .collect();
    let mut list_state = ListState::default().with_selected(Some(settings.row));
    let list = List::new(items).highlight_style(Style::new().add_modifier(Modifier::REVERSED));
    frame.render_stateful_widget(list, area, &mut list_state);
    let first = list_state.offset();
    for (i, y) in (area.y..area.bottom()).enumerate() {
        if first + i < rows.len() {
            hits.push(
                Rect::new(area.x, y, area.width, 1),
                Target::SettingsRow(first + i),
            );
        }
    }
}

/// One row: the label, then its value, what is being typed, or the removal question.
fn row_line(row: &SRow, selected: bool, state: &AppState) -> Line<'static> {
    let settings = &state.settings;
    let label = format!(" {:<LABEL$}", row.label);
    let dim = Style::new().add_modifier(Modifier::DIM);
    let label_style = if row.na {
        dim
    } else {
        Style::new().add_modifier(Modifier::BOLD)
    };
    let value = match (&settings.field, selected && row.field) {
        (Some(text), true) => Span::raw(format!("\u{203a} {text}\u{258f}")),
        _ if selected && settings.confirm == Some(settings.row) => Span::styled(
            "press d again to remove",
            Style::new().add_modifier(Modifier::BOLD),
        ),
        _ if row.na => Span::styled(format!("not in a terminal: {}", row.value), dim),
        _ if row.act.is_some() => Span::styled(
            row.value.clone(),
            Style::new().add_modifier(Modifier::UNDERLINED),
        ),
        _ => Span::raw(row.value.clone()),
    };
    Line::from(vec![Span::styled(label, label_style), value])
}
