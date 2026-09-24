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

use crate::hit::{HitMap, Target};
use crate::keymap::Command;
use crate::state::AppState;
use crate::state_detail::{Level, Part, swap_in};
use crate::ui::{header::workspace_name, notes_edit, row};

/// Draws the panel into `area` and records its targets.
pub fn draw(frame: &mut Frame, area: Rect, state: &AppState, hits: &mut HitMap) {
    let Some(level) = state.detail.top() else {
        return;
    };
    let block = Block::default().borders(Borders::TOP);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    hits.push(area, Target::Inert);
    let [crumbs, parent, sub_label, sub, notes_label, notes] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Fill(1),
        Constraint::Length(1),
        Constraint::Fill(1),
    ])
    .areas(inner);
    draw_crumbs(frame, crumbs, state, hits);
    draw_parent(frame, parent, state, level, hits);
    let part = state.detail.part;
    let sub_text = if level.doc.lines.is_empty() {
        "Sub-list \u{b7} none yet: a adds the first sub-task".to_owned()
    } else {
        format!("Sub-list \u{b7} {}", level.doc.path)
    };
    label(frame, sub_label, &sub_text, part == Part::Sub);
    draw_sub_list(frame, sub, state, hits);
    label(
        frame,
        notes_label,
        &format!("Notes \u{b7} {}/notes.md", level.dir),
        part == Part::Notes,
    );
    draw_notes(frame, notes, level, part == Part::Notes);
    hits.push(notes, Target::Command(Command::DetailEditNotes));
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

/// The parent line, or its draft with a caret while the field has the keyboard, and "Mark done".
fn draw_parent(frame: &mut Frame, area: Rect, state: &AppState, level: &Level, hits: &mut HitMap) {
    hits.push(area, Target::Command(Command::DetailEditParent));
    let mut spans = vec![Span::styled(
        " Parent  ",
        Style::new().add_modifier(Modifier::DIM),
    )];
    match &level.parent_draft {
        Some(draft) => {
            let (before, after) = draft.buffer.split_at(draft.caret);
            spans.push(Span::raw(before.to_owned()));
            spans.push(Span::styled(
                "\u{258f}",
                Style::new().add_modifier(Modifier::BOLD),
            ));
            spans.push(Span::raw(after.to_owned()));
        }
        None => spans.extend(
            crate::paint::paint_line(&level.parent.raw, level.parent.completed, false).spans,
        ),
    }
    if !level.parent.completed && crate::commands_detail::all_sub_tasks_done(state) {
        let button = " Mark done ";
        let width = u16::try_from(button.len()).unwrap_or(0);
        let rect = Rect::new(area.right().saturating_sub(width), area.y, width, 1);
        frame.render_widget(
            Paragraph::new(button).style(Style::new().add_modifier(Modifier::REVERSED)),
            rect,
        );
        hits.push(rect, Target::Command(Command::DetailCompleteParent));
        let text_area = Rect::new(area.x, area.y, area.width.saturating_sub(width + 1), 1);
        frame.render_widget(Paragraph::new(Line::from(spans)), text_area);
        return;
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
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
    let items: Vec<ListItem> = (0..=view.lines.len())
        .map(|i| ListItem::new(row::paint(&view, i)))
        .collect();
    let focused = state.detail.part == Part::Sub;
    let highlight = if focused {
        Style::new().add_modifier(Modifier::REVERSED)
    } else {
        Style::new()
    };
    let mut list_state = ListState::default()
        .with_offset(view.scroll)
        .with_selected(Some(view.cursor));
    frame.render_stateful_widget(
        List::new(items).highlight_style(highlight),
        area,
        &mut list_state,
    );
    let first = list_state.offset();
    for (i, y) in (area.y..area.bottom()).enumerate() {
        if first + i <= view.lines.len() {
            hits.push(
                Rect::new(area.x, y, area.width, 1),
                Target::DetailRow(first + i),
            );
        }
    }
}

/// The notes, a caret drawn while they have the keyboard; the view follows the caret's line.
fn draw_notes(frame: &mut Frame, area: Rect, level: &Level, focused: bool) {
    let (caret_line, caret_col) = notes_edit::caret_position(&level.notes);
    let height = usize::from(area.height.max(1));
    let top = caret_line.saturating_sub(height - 1);
    let lines: Vec<Line> = level
        .notes
        .text
        .split('\n')
        .enumerate()
        .skip(top)
        .take(height)
        .map(|(n, text)| {
            if !(focused && n == caret_line) {
                return Line::raw(format!(" {text}"));
            }
            let split = text
                .char_indices()
                .nth(caret_col)
                .map_or(text.len(), |(i, _)| i);
            let (before, after) = text.split_at(split);
            Line::from(vec![
                Span::raw(format!(" {before}")),
                Span::styled("\u{258f}", Style::new().add_modifier(Modifier::BOLD)),
                Span::raw(after.to_owned()),
            ])
        })
        .collect();
    let placeholder = level.notes.text.is_empty() && !focused;
    let body = if placeholder {
        Paragraph::new(" No notes yet. Tab here to write some.")
            .style(Style::new().add_modifier(Modifier::DIM))
    } else {
        Paragraph::new(lines)
    };
    frame.render_widget(body, area);
}

#[cfg(test)]
#[path = "detail_tests.rs"]
mod tests;
