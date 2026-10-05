//! The detail panel (task `tui-revamp/tui-detail`, c2 `c2-prompt.html:144-148`): the bottom 55% of
//! the Tasks screen, the list still above it. Top to bottom: the breadcrumb (workspace, then each
//! level's parent, each a click target), the parent line (a field while it has the keyboard, with
//! "Mark done" once every sub-task is), the sub-list in list mode, and the notes. The part with
//! the keyboard has its label reversed.
//! Ref: <https://docs.rs/ratatui/latest/ratatui/layout/struct.Layout.html>

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph};

use crate::hit::{Field, HitMap, Target};
use crate::keymap::Command;
use crate::state::AppState;
use crate::state_detail::{Level, Part, swap_in};
use crate::state_nav::Focus;
use crate::ui::{field, header::workspace_name, notes_wrap, row, row_edit};

/// Draws the panel into `area` and records its targets.
pub fn draw(frame: &mut Frame, area: Rect, state: &AppState, hits: &mut HitMap) {
    let Some(level) = state.detail.top() else {
        return;
    };
    // The top border is the panel's edge: the accent, bold, while keys go to the panel, so it is
    // plain which half of the screen typing lands in.
    let edge = if panel_has_keys(state) {
        Style::new()
            .fg(crate::theme::current().accent())
            .add_modifier(Modifier::BOLD)
    } else {
        Style::new().add_modifier(Modifier::DIM)
    };
    let block = Block::default().borders(Borders::TOP).border_style(edge);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    hits.push(area, Target::Inert);
    // `v`: the parent's fields, one per row, under the parent line.
    let fields = if state.detail.breakdown {
        txtodo_core::breakdown::fields(&level.parent.raw)
    } else {
        Vec::new()
    };
    let parent_rows = 1 + u16::try_from(fields.len()).unwrap_or(0);
    let [crumbs, parent_area, sub_label, sub, notes_label, notes] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(parent_rows),
        Constraint::Length(1),
        Constraint::Fill(1),
        Constraint::Length(1),
        Constraint::Fill(1),
    ])
    .areas(inner);
    draw_crumbs(frame, crumbs, state, hits);
    let parent = Rect {
        height: 1.min(parent_area.height),
        ..parent_area
    };
    draw_parent(frame, parent, state, level, hits);
    draw_breakdown(frame, parent_area, &fields);
    let part = state.detail.part;
    let sub_text = if level.doc.lines.is_empty() {
        "Sub-list \u{b7} none yet: a adds the first sub-task".to_owned()
    } else {
        format!("Sub-list \u{b7} {}", level.doc.path)
    };
    let keys = panel_has_keys(state);
    label(frame, sub_label, &sub_text, keys && part == Part::Sub);
    draw_sub_list(frame, sub, state, hits);
    label(
        frame,
        notes_label,
        &format!("Notes \u{b7} {}/notes.md", level.dir),
        keys && part == Part::Notes,
    );
    draw_notes(
        frame,
        notes,
        level,
        part == Part::Notes && panel_has_keys(state),
    );
    hits.push(notes, Target::Field(Field::Notes, notes));
}

/// A part's label, reversed while it has the keyboard.
fn label(frame: &mut Frame, area: Rect, text: &str, focused: bool) {
    let style = if focused {
        Style::new().add_modifier(Modifier::REVERSED | Modifier::BOLD)
    } else {
        Style::new().add_modifier(Modifier::DIM)
    };
    frame.render_widget(
        Paragraph::new(Span::styled(format!(" {text} "), style)),
        area,
    );
}

/// `workspace › parent › parent`: the workspace closes the panel, a parent cuts back to its level.
fn draw_crumbs(frame: &mut Frame, area: Rect, state: &AppState, hits: &mut HitMap) {
    let mut spans = Vec::new();
    let mut x = area.x;
    let crumbs = state.detail.crumbs();
    let names = std::iter::once(workspace_name(state)).chain(crumbs.iter().cloned());
    for (i, name) in names.enumerate() {
        if i > 0 {
            spans.push(Span::styled(
                " \u{203a} ",
                Style::new().add_modifier(Modifier::DIM),
            ));
            x += 3;
        }
        let text = format!(" {name}");
        let width = u16::try_from(Span::raw(text.as_str()).width()).unwrap_or(0);
        let target = if i == 0 {
            Target::Command(Command::DetailClose)
        } else {
            Target::Crumb(i)
        };
        hits.push(
            Rect::new(x, area.y, width.min(area.right().saturating_sub(x)), 1),
            target,
        );
        let last = i == crumbs.len();
        let style = if last {
            Style::new().add_modifier(Modifier::BOLD)
        } else {
            Style::new().add_modifier(Modifier::UNDERLINED)
        };
        spans.push(Span::styled(text, style));
        x += width;
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

/// The parent line, or its draft as a field while it has the keyboard, and "Mark done".
fn draw_parent(frame: &mut Frame, area: Rect, state: &AppState, level: &Level, hits: &mut HitMap) {
    hits.push(area, Target::Command(Command::DetailEditParent));
    let label = Span::styled(" Parent  ", Style::new().add_modifier(Modifier::DIM));
    let mut text_area = area;
    if !level.parent.completed && crate::commands_detail::all_sub_tasks_done(state) {
        let button = " Mark done ";
        let width = u16::try_from(button.len()).unwrap_or(0);
        let rect = Rect::new(area.right().saturating_sub(width), area.y, width, 1);
        frame.render_widget(
            Paragraph::new(button).style(Style::new().add_modifier(Modifier::REVERSED)),
            rect,
        );
        hits.push(rect, Target::Command(Command::DetailCompleteParent));
        text_area.width = area.width.saturating_sub(width + 1);
    }
    let label_width = u16::try_from(label.width())
        .unwrap_or(0)
        .min(text_area.width);
    let field_area = Rect {
        x: text_area.x + label_width,
        width: text_area.width - label_width,
        ..text_area
    };
    // The text itself is a field target, so a click lands the caret on the character under it.
    hits.push(field_area, Target::Field(Field::Parent, field_area));
    match &level.parent_draft {
        Some(draft) => {
            frame.render_widget(Paragraph::new(Line::from(label)), text_area);
            let style = crate::theme::current().input_style();
            let cell = field::draw(frame, field_area, &draft.buffer, draft.caret, style);
            if panel_has_keys(state) {
                // The label's trailing space takes the edge mark, as an edited row's gutter does.
                row_edit::draw_mark(frame, field_area);
                frame.set_cursor_position(cell);
            }
        }
        None => {
            let mut spans = vec![label];
            let shown = crate::state_tasks::without_ref(&level.parent.raw);
            spans.extend(crate::paint::paint_line(&shown, level.parent.completed, false).spans);
            frame.render_widget(Paragraph::new(Line::from(spans)), text_area);
        }
    }
}

/// The parent's fields under its row, `name = value` with the names right-aligned, sqlite
/// `.mode line` style (core `breakdown::fields`).
fn draw_breakdown(frame: &mut Frame, area: Rect, fields: &[(String, String)]) {
    let width = fields
        .iter()
        .map(|(n, _)| n.chars().count())
        .max()
        .unwrap_or(0);
    let dim = Style::new().add_modifier(Modifier::DIM);
    let lines: Vec<Line> = fields
        .iter()
        .map(|(name, value)| {
            Line::from(vec![
                Span::styled(format!("   {name:>width$} = "), dim),
                Span::raw(value.clone()),
            ])
        })
        .collect();
    let below = Rect {
        y: area.y + 1,
        height: area.height.saturating_sub(1),
        ..area
    };
    frame.render_widget(Paragraph::new(lines), below);
}

/// Whether keys go to the panel at all (its part then says which field).
fn panel_has_keys(state: &AppState) -> bool {
    state.nav.focus == Focus::Detail
}

/// The sub-list's rows, painted as the root list's are, the cursor reversed while it has the
/// keyboard. Each row is a click target.
fn draw_sub_list(frame: &mut Frame, area: Rect, state: &AppState, hits: &mut HitMap) {
    let Some(level) = state.detail.top() else {
        return;
    };
    // Paint through a copy with the sub-list swapped in: rows read the list off `AppState`.
    let mut view = state.clone();
    let mut doc = level.doc.clone();
    swap_in(&mut view, &mut doc);
    view.tasks.refs.clear();
    let items: Vec<ListItem> = (0..view.row_count())
        .map(|r| ListItem::new(row::paint(&view, view.index_at_row(r))))
        .collect();
    let highlight =
        crate::ui::list::selection_style(state.detail.part == Part::Sub && panel_has_keys(state));
    let mut list_state = ListState::default()
        .with_offset(view.scroll)
        .with_selected(Some(view.row_of(view.cursor)));
    frame.render_stateful_widget(
        List::new(items).highlight_style(highlight),
        area,
        &mut list_state,
    );
    let first = list_state.offset();
    for (i, y) in (area.y..area.bottom()).enumerate() {
        if first + i < view.row_count() {
            hits.push(
                Rect::new(area.x, y, area.width, 1),
                Target::DetailRow(view.index_at_row(first + i)),
            );
        }
    }
    // A sub-line being edited: the line editor runs on the swapped-in sub-list, so draw it here.
    if let (Some(draft), true) = (&state.editing, panel_has_keys(state)) {
        let rows = row_edit::Rows::of(&view, area, first);
        if let Some(field) = row_edit::draw(frame, rows, view.row_of(view.cursor), draft) {
            hits.push(field, Target::Field(Field::LineEdit, field));
        }
    }
}

/// The notes, soft-wrapped to the panel, tinted with the real cursor on the caret while they have
/// the keyboard; the view follows the caret's row.
fn draw_notes(frame: &mut Frame, area: Rect, level: &Level, focused: bool) {
    let notes = &level.notes;
    if notes.text.is_empty() && !focused {
        frame.render_widget(
            Paragraph::new(" No notes yet. Tab here to write some.")
                .style(Style::new().add_modifier(Modifier::DIM)),
            area,
        );
        return;
    }
    // One cell of margin at the left, and one spare at the right so a caret at a full row's end
    // still has a cell.
    let width = area.width.saturating_sub(2).max(1);
    notes.wrap_width.set(width);
    let rows = notes_wrap::rows(&notes.text, usize::from(width));
    let (caret_row, caret_col) = notes_wrap::caret_cell(&notes.text, &rows, notes.caret);
    let height = usize::from(area.height.max(1));
    let top = caret_row.saturating_sub(height - 1);
    let lines: Vec<Line> = rows
        .iter()
        .skip(top)
        .take(height)
        .map(|r| Line::raw(format!(" {}", &notes.text[r.clone()])))
        .collect();
    let style = if focused {
        crate::theme::current().input_style()
    } else {
        Style::new()
    };
    frame.render_widget(Paragraph::new(lines).style(style), area);
    if focused && area.width > 0 && area.height > 0 {
        let column = u16::try_from(caret_col + 1)
            .unwrap_or(u16::MAX)
            .min(area.width - 1);
        let row = u16::try_from(caret_row - top).unwrap_or(0);
        frame.set_cursor_position((area.x + column, area.y + row));
    }
}

#[cfg(test)]
#[path = "detail_tests.rs"]
mod tests;
