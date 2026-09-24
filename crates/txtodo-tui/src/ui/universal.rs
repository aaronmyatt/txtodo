//! The Universal screen (task `tui-revamp/tui-universal`, c2 `c2/universal.js`): open tasks from
//! every workspace. Top to bottom: a stat strip, the grouping and "show done", the workspace
//! chips, the context chips, then the rows under their group headings: a done mark, the priority
//! (when not grouped by it), the painted line, the due badge, the `ref:` pill and a two-letter
//! workspace tile. Every chip, toggle and row is a click target; the header search narrows the
//! rows.
//! Ref: <https://docs.rs/ratatui/latest/ratatui/widgets/struct.List.html>

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{List, ListItem, ListState, Paragraph};
use txtodo_core::universal::{GroupBy, due_label};

use crate::commands::today_local;
use crate::hit::{HitMap, Target};
use crate::keymap::Command;
use crate::state::AppState;
use crate::state_universal::{GROUPS, UTask};

/// A row of clickable chips: text, whether it is on, and its target.
type Chips = Vec<(String, bool, Target)>;

/// Draws the screen into `area` and records its targets.
pub fn draw(frame: &mut Frame, area: Rect, state: &AppState, hits: &mut HitMap) {
    let today = today_local();
    let view = &state.universal;
    let query = &state.shell.search;
    let [stats, groups, workspaces, contexts, list] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(1),
    ])
    .areas(area);
    frame.render_widget(Paragraph::new(stat_line(view.stats(&today))), stats);
    let mut group_chips: Chips = GROUPS
        .iter()
        .enumerate()
        .map(|(i, (g, name))| {
            (
                (*name).to_owned(),
                *g == view.group,
                Target::UniversalGroup(i),
            )
        })
        .collect();
    let mark = if view.show_done { "[x]" } else { "[ ]" };
    group_chips.push((
        format!("{mark} show done"),
        false,
        Target::Command(Command::UniversalShowDone),
    ));
    chip_row(frame, groups, "Group", group_chips, hits);
    let ws_chips: Chips = view
        .workspaces()
        .into_iter()
        .enumerate()
        .map(|(i, (id, name, n))| {
            (
                format!("{name} {n}"),
                !view.hidden.contains(&id),
                Target::UniversalWorkspace(i),
            )
        })
        .collect();
    chip_row(frame, workspaces, "Workspaces", ws_chips, hits);
    let ctx_chips: Chips = view
        .contexts(query)
        .into_iter()
        .enumerate()
        .map(|(i, (c, n))| {
            let on = view.context.as_deref() == Some(c.as_str());
            (format!("@{c} {n}"), on, Target::UniversalContext(i))
        })
        .collect();
    chip_row(frame, contexts, "Contexts", ctx_chips, hits);
    draw_rows(frame, list, state, &today, hits);
}

/// `Universal  2 overdue  3 due this week  t 14 open  x 6 done`.
fn stat_line([overdue, week, open, done]: [usize; 4]) -> Line<'static> {
    let dim = Style::new().add_modifier(Modifier::DIM);
    let bold = Style::new().add_modifier(Modifier::BOLD);
    Line::from(vec![
        Span::styled(" Universal  ", bold),
        Span::styled(format!("{overdue}"), bold),
        Span::styled(" overdue  ", dim),
        Span::styled(format!("{week}"), bold),
        Span::styled(" due this week  ", dim),
        Span::styled("t ", bold),
        Span::styled(format!("{open} open  "), dim),
        Span::styled("x ", bold),
        Span::styled(format!("{done} done"), dim),
    ])
}

/// A dim heading, then chips (reversed when on), as many as fit, each recording its target.
fn chip_row(frame: &mut Frame, area: Rect, head: &str, chips: Chips, hits: &mut HitMap) {
    let head = format!(" {head:<11}");
    let mut x = area.x + u16::try_from(head.len()).unwrap_or(0);
    let mut spans = vec![Span::styled(head, Style::new().add_modifier(Modifier::DIM))];
    for (text, on, target) in chips {
        let chip = format!(" {text} ");
        let width = u16::try_from(Span::raw(chip.as_str()).width()).unwrap_or(0);
        if x + width > area.right() {
            break;
        }
        hits.push(Rect::new(x, area.y, width, 1), target);
        let style = if on {
            Style::new().add_modifier(Modifier::REVERSED)
        } else {
            Style::new().add_modifier(Modifier::UNDERLINED)
        };
        spans.push(Span::styled(chip, style));
        spans.push(Span::raw(" "));
        x += width + 1;
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

/// The group headings and rows, the selected row reversed; or the empty state.
fn draw_rows(frame: &mut Frame, area: Rect, state: &AppState, today: &str, hits: &mut HitMap) {
    let view = &state.universal;
    let groups = view.groups(&state.shell.search, today);
    if groups.is_empty() {
        let text = " Nothing matches. Loosen the search or the filters.  ";
        frame.render_widget(Paragraph::new(text), area);
        let x = area.x + u16::try_from(text.len()).unwrap_or(0);
        let button = Rect::new(x, area.y, 15.min(area.right().saturating_sub(x)), 1);
        frame.render_widget(
            Paragraph::new(" Reset filters ").style(Style::new().add_modifier(Modifier::REVERSED)),
            button,
        );
        hits.push(button, Target::Command(Command::UniversalReset));
        return;
    }
    let mut items = Vec::new();
    let mut selected_item = 0;
    let mut row_of_item = Vec::new();
    let mut row = 0;
    for (name, members) in &groups {
        items.push(ListItem::new(Line::styled(
            format!(" {name} \u{b7} {}", members.len()),
            Style::new().add_modifier(Modifier::BOLD),
        )));
        row_of_item.push(None);
        for &i in members {
            if row == view.cursor {
                selected_item = items.len();
            }
            items.push(ListItem::new(row_line(&view.tasks[i], view.group, today)));
            row_of_item.push(Some(row));
            row += 1;
        }
    }
    let mut list_state = ListState::default().with_selected(Some(selected_item));
    let list = List::new(items).highlight_style(Style::new().add_modifier(Modifier::REVERSED));
    frame.render_stateful_widget(list, area, &mut list_state);
    let first = list_state.offset();
    for (i, y) in (area.y..area.bottom()).enumerate() {
        if let Some(Some(row)) = row_of_item.get(first + i) {
            hits.push(
                Rect::new(area.x, y, area.width, 1),
                Target::UniversalRow(*row),
            );
        }
    }
}

/// One task: mark, the painted line, due badge, `ref:` pill, and a workspace tile (unless grouped
/// by workspace).
fn row_line(task: &UTask, by: GroupBy, today: &str) -> Line<'static> {
    let dim = Style::new().add_modifier(Modifier::DIM);
    let mark = if task.done {
        "  \u{2713} "
    } else {
        "  \u{25cb} "
    };
    let mut spans = vec![Span::raw(mark)];
    spans.extend(crate::paint::paint_line(&task.raw, task.done, false).spans);
    if let Some(badge) = due_badge(task, today) {
        spans.push(Span::raw("  "));
        spans.push(badge);
    }
    match (task.progress, task.has_notes) {
        (Some((done, total)), _) => spans.push(Span::styled(format!("  {done}/{total}"), dim)),
        (None, true) => spans.push(Span::styled("  \u{b6}", dim)),
        (None, false) => {}
    }
    if by != GroupBy::Workspace {
        let tile: String = task.workspace.chars().take(2).collect();
        spans.push(Span::styled(format!("  [{tile}]"), dim));
    }
    Line::from(spans)
}

/// An open task's due badge (core `due_label`): reversed when due today or overdue.
fn due_badge(task: &UTask, today: &str) -> Option<Span<'static>> {
    if task.done {
        return None;
    }
    let (text, days) = due_label(task.due.as_deref(), today)?;
    let loud = if days <= 0 {
        Modifier::BOLD | Modifier::REVERSED
    } else {
        Modifier::BOLD
    };
    Some(Span::styled(
        format!(" {text} "),
        Style::new().add_modifier(loud),
    ))
}

#[cfg(test)]
#[path = "universal_tests.rs"]
mod tests;
