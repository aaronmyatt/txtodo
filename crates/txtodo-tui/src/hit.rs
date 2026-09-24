//! The hit map (task `tui-revamp/tui-mouse`): every frame, `ui::screen::draw` records where each
//! clickable thing landed, so a mouse event can be looked up without redrawing. Later rects sit on
//! top of earlier ones, the same as the drawing order: an overlay drawn over the list hides the rows
//! underneath from the mouse too.
//! Ref: <https://docs.rs/ratatui/latest/ratatui/layout/struct.Rect.html>

use ratatui::layout::{Position, Rect};

use crate::keymap::Command;

/// What a cell of the screen is, to the mouse.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    /// A row of the Tasks list: a line index, or `lines.len()` for the Add-a-line row.
    Row(usize),
    /// A clickable that runs a command: a tab, a button, a chip.
    Command(Command),
    /// A row of the `W` popup: a workspace, or `items.len()` for Manage workspaces.
    MenuItem(usize),
    /// An overlay's body: it takes the click, and nothing underneath gets it.
    Inert,
}

/// Where the Tasks list was drawn, and which row sat at its top.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ListView {
    /// The rows' area on screen.
    pub area: Rect,
    /// The index of the row at the top of `area` (ratatui's `ListState::offset`).
    pub offset: usize,
}

/// One frame's clickable areas, in drawing order.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HitMap {
    targets: Vec<(Rect, Target)>,
    list: Option<ListView>,
}

impl HitMap {
    /// Records `target` over `area`, on top of everything recorded before it.
    pub fn push(&mut self, area: Rect, target: Target) {
        self.targets.push((area, target));
    }

    /// Records the Tasks list: one-cell-high rows from `offset`, `rows` of them in all.
    pub fn record_list(&mut self, area: Rect, offset: usize, rows: usize) {
        self.list = Some(ListView { area, offset });
        let visible = rows.saturating_sub(offset).min(usize::from(area.height));
        for (i, y) in (area.y..).take(visible).enumerate() {
            self.push(Rect::new(area.x, y, area.width, 1), Target::Row(offset + i));
        }
    }

    /// Where the Tasks list was, if it was drawn.
    pub fn list(&self) -> Option<ListView> {
        self.list
    }

    /// The topmost target under the cell at (`column`, `row`).
    pub fn at(&self, column: u16, row: u16) -> Option<Target> {
        let cell = Position::new(column, row);
        self.targets
            .iter()
            .rev()
            .find(|(area, _)| area.contains(cell))
            .map(|(_, target)| *target)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_map_from_the_scroll_offset_and_stop_at_the_last_row() {
        let mut hits = HitMap::default();
        hits.record_list(Rect::new(0, 2, 40, 5), 3, 6);
        assert_eq!(hits.at(0, 2), Some(Target::Row(3)), "top row is the offset");
        assert_eq!(hits.at(39, 4), Some(Target::Row(5)));
        assert_eq!(hits.at(0, 5), None, "only rows 3..6 exist");
        assert_eq!(hits.at(40, 2), None, "past the right edge");
        assert_eq!(hits.list().map(|l| l.offset), Some(3));
    }

    #[test]
    fn a_later_rect_covers_an_earlier_one() {
        let mut hits = HitMap::default();
        hits.record_list(Rect::new(0, 0, 40, 10), 0, 10);
        hits.push(Rect::new(0, 9, 40, 1), Target::Inert);
        assert_eq!(hits.at(5, 9), Some(Target::Inert));
        assert_eq!(hits.at(5, 8), Some(Target::Row(8)));
    }
}
