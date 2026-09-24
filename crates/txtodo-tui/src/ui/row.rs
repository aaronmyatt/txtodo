//! One row of the Tasks list (task `tui-revamp/tui-tasks`, implementation plan §3.1): a dim
//! line-number gutter, the painted line (done lines dim and struck), a `ref:` badge (`n/m`, or a
//! notes mark), text past the 100-char hint underlined, and, while searching, hits marked and
//! misses faded. Marks are laid on the text as drawn, char by char.
//! Ref: <https://docs.rs/ratatui/latest/ratatui/text/struct.Span.html>

use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use txtodo_core::{LINE_LENGTH_HINT, over_length_hint};

use crate::paint::paint_line;
use crate::state::AppState;
use crate::state_tasks::{RefBadge, ref_slug};

/// The Add-a-line row's text (design §3.1).
pub const ADD_LINE_PLACEHOLDER: &str = "+ Add a line";

/// The notes mark, for a `ref:` directory with only a `notes.md`.
const NOTES_MARK: &str = "\u{b6}";

/// The gutter's width: the widest line number, and a space.
pub fn gutter_width(state: &AppState) -> usize {
    state.lines.len().max(1).to_string().len() + 1
}

/// Row `i` of the list: a line, or the Add-a-line row at `lines.len()`.
pub fn paint(state: &AppState, i: usize) -> Line<'static> {
    let width = gutter_width(state);
    let dim = Style::new().add_modifier(Modifier::DIM);
    let Some(line) = state.lines.get(i) else {
        return Line::from(vec![
            Span::raw(" ".repeat(width)),
            Span::styled(ADD_LINE_PLACEHOLDER, dim.add_modifier(Modifier::ITALIC)),
        ]);
    };
    let mut body = paint_line(&line.raw, line.completed, false);
    if over_length_hint(&line.raw).is_some() {
        body = mark(
            body,
            &[(LINE_LENGTH_HINT, usize::MAX)],
            Modifier::UNDERLINED,
        );
    }
    let query = &state.shell.search;
    if crate::search::active(query) && !line.raw.trim().is_empty() {
        body = if crate::search::is_hit(&line.raw, query) {
            let text: String = body.spans.iter().map(|s| s.content.as_ref()).collect();
            mark(
                body,
                &crate::search::ranges(&text, query),
                Modifier::REVERSED,
            )
        } else {
            Line::from(
                body.spans
                    .into_iter()
                    .map(|s| s.patch_style(dim))
                    .collect::<Vec<_>>(),
            )
        };
    }
    let mut spans = vec![Span::styled(
        format!("{:>w$} ", line.line_number, w = width - 1),
        dim,
    )];
    spans.extend(body.spans);
    if let Some(badge) = ref_slug(&line.raw).and_then(|s| state.tasks.refs.get(s)) {
        let text = match badge {
            RefBadge::Progress { done, total } => format!(" {done}/{total}"),
            RefBadge::Notes => format!(" {NOTES_MARK}"),
        };
        spans.push(Span::styled(text, dim));
    }
    Line::from(spans)
}

/// `line` with `modifier` added over the char ranges `[start, end)`, splitting spans at the edges.
pub fn mark(line: Line<'static>, ranges: &[(usize, usize)], modifier: Modifier) -> Line<'static> {
    if ranges.is_empty() {
        return line;
    }
    let inside = |at: usize| ranges.iter().any(|&(s, e)| s <= at && at < e);
    let mut out = Vec::new();
    let mut at = 0;
    for span in line.spans {
        let mut piece = String::new();
        let mut piece_marked = None;
        for c in span.content.chars() {
            let marked = inside(at);
            if piece_marked.is_some_and(|m| m != marked) {
                out.push(styled(
                    std::mem::take(&mut piece),
                    span.style,
                    piece_marked,
                    modifier,
                ));
            }
            piece_marked = Some(marked);
            piece.push(c);
            at += 1;
        }
        if !piece.is_empty() {
            out.push(styled(piece, span.style, piece_marked, modifier));
        }
    }
    Line::from(out).style(line.style)
}

fn styled(text: String, style: Style, marked: Option<bool>, modifier: Modifier) -> Span<'static> {
    if marked == Some(true) {
        Span::styled(text, style.add_modifier(modifier))
    } else {
        Span::styled(text, style)
    }
}

#[cfg(test)]
#[path = "row_tests.rs"]
mod tests;
