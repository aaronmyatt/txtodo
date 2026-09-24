//! Toasts (task `tui-revamp/tui-shell`, c2 `c2-prompt.html:189-199`): short notes stacked in the
//! bottom-right corner of the screen for [`TOAST_FOR`], the newest lowest. The newest one carries
//! Undo when it reports a change; Undo goes through the daemon's `Undo`, never a local re-toggle.
//! Ref: <https://docs.rs/ratatui/latest/ratatui/widgets/struct.Clear.html>

use std::time::Instant;

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph};

use crate::hit::{HitMap, Target};
use crate::keymap::Command;
use crate::state_shell::{Shell, TOAST_FOR};

/// The most toasts shown at once.
const MAX_SHOWN: usize = 3;

/// Draws the live toasts over the bottom-right of `area`, and records Undo's cells.
pub fn draw(frame: &mut Frame, area: Rect, shell: &Shell, now: Instant, hits: &mut HitMap) {
    let live: Vec<_> = shell
        .toasts
        .iter()
        .filter(|t| now.duration_since(t.at) < TOAST_FOR)
        .collect();
    let shown = live.iter().rev().take(MAX_SHOWN);
    for (i, (toast, y)) in shown.zip((area.y..area.bottom()).rev()).enumerate() {
        // Only the newest toast undoes, and only while its change is the newest one.
        let undo = i == 0 && shell.undoable().is_some();
        let text = format!(" {} ", toast.message);
        let button = if undo { " Undo " } else { "" };
        let width = u16::try_from(Span::raw(text.as_str()).width() + button.len())
            .unwrap_or(u16::MAX)
            .min(area.width);
        let rect = Rect::new(area.right() - width, y, width, 1);
        frame.render_widget(Clear, rect);
        let line = Line::from(vec![
            Span::styled(text, Style::new().add_modifier(Modifier::REVERSED)),
            Span::styled(button, Style::new().add_modifier(Modifier::BOLD)),
        ]);
        frame.render_widget(Paragraph::new(line), rect);
        hits.push(rect, Target::Inert);
        if undo {
            let button_width = u16::try_from(button.len()).unwrap_or(0).min(width);
            hits.push(
                Rect::new(rect.right() - button_width, y, button_width, 1),
                Target::Command(Command::ToastUndo),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    #[test]
    fn toasts_stack_bottom_right_and_only_the_newest_has_undo() {
        let now = Instant::now();
        let mut shell = Shell::default();
        shell.toast("Old", None, now - TOAST_FOR);
        let first = shell.record("todo.txt", 1);
        shell.toast("Completed the line", Some(first), now);
        let second = shell.record("todo.txt", 2);
        shell.toast("Deleted the line", Some(second), now);
        let mut terminal = Terminal::new(TestBackend::new(40, 4)).unwrap_or_else(|e| panic!("{e}"));
        let mut hits = HitMap::default();
        terminal
            .draw(|f| draw(f, f.area(), &shell, now, &mut hits))
            .unwrap_or_else(|e| panic!("{e}"));
        let rows: Vec<String> = terminal
            .backend()
            .buffer()
            .content()
            .chunks(40)
            .map(|r| r.iter().map(|c| c.symbol()).collect())
            .collect();
        assert!(rows[3].ends_with(" Deleted the line  Undo "), "{rows:?}");
        assert!(rows[2].ends_with(" Completed the line "), "{rows:?}");
        assert!(!rows.iter().any(|r| r.contains("Old")), "expired");
        assert_eq!(hits.at(38, 3), Some(Target::Command(Command::ToastUndo)));
        assert_eq!(
            hits.at(38, 2),
            Some(Target::Inert),
            "an older toast has no Undo"
        );
    }
}
