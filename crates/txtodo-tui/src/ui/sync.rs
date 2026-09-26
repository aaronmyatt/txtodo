//! The `s` sync popup: renders a [`SyncSnapshot`] (peers with their lag, and pending ops) as a
//! summary line and one row per peer (design §7; a popup from the footer since task
//! `tui-revamp/tui-shell`). Pure rendering against the mock/fixture snapshot for now
//! (recommended build order step 3); `app.rs` refreshes `AppState.sync` from the real
//! `SyncStatus` RPC on a 1 s tick plus every `Watch` event once that RPC exists (step 4).

use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::state::{PeerStatus, SyncSnapshot};

/// Renders the indicator line: `● synced` with no peers and nothing pending; otherwise
/// `● N peer(s) · lag <max lag>ms · P pending`, plus `· S stuck` when a peer's ops keep being
/// refused (task sync-drift line 7). Colour signals convergence (green = nothing pending, yellow =
/// ops still in flight, red = stuck) but is never the only signal (plan §3.3): the text itself
/// always states the peer, pending and stuck counts.
pub fn render(sync: &SyncSnapshot) -> Line<'static> {
    let stuck: usize = sync.peers.iter().map(|p| p.stuck.len()).sum();
    let dot_color = if stuck > 0 {
        Color::Red
    } else if sync.pending_ops == 0 {
        Color::Green
    } else {
        Color::Yellow
    };
    let mut spans = vec![Span::styled("\u{25cf} ", Style::new().fg(dot_color))];
    if sync.peers.is_empty() {
        spans.push(Span::raw("no peers"));
    } else {
        let max_lag = sync.peers.iter().map(|p| p.lag_ms).max().unwrap_or(0);
        spans.push(Span::raw(format!(
            "{} peer{} \u{b7} lag {}ms",
            sync.peers.len(),
            if sync.peers.len() == 1 { "" } else { "s" },
            max_lag
        )));
    }
    spans.push(Span::raw(format!(" \u{b7} {} pending", sync.pending_ops)));
    if stuck > 0 {
        spans.push(Span::raw(format!(" \u{b7} {stuck} stuck")));
    }
    Line::from(spans)
}

/// One peer's rows: its lag, then each file its ops are stuck on and whether it is parked.
fn peer_lines(p: &PeerStatus) -> Vec<Line<'static>> {
    let short: String = p.device.chars().take(10).collect();
    let mut lines = vec![Line::from(format!("  {short}\u{2026}  lag {}ms", p.lag_ms))];
    lines.extend(
        p.stuck
            .iter()
            .map(|f| Line::from(format!("    stuck on {f}"))),
    );
    if p.parked {
        lines.push(Line::from("    parked: no shared key"));
    }
    lines
}

/// The sync popup (task `tui-revamp/tui-shell`, replacing the one-line `s` pane): the summary line,
/// then one row per peer with its lag, in a box over the bottom-left of `area`.
/// Ref: <https://docs.rs/ratatui/latest/ratatui/widgets/struct.Clear.html>
pub fn draw_popup(
    frame: &mut ratatui::Frame,
    area: ratatui::layout::Rect,
    sync: &SyncSnapshot,
    hits: &mut crate::hit::HitMap,
) {
    use ratatui::widgets::{Block, Borders, Clear};
    let mut lines = vec![render(sync)];
    lines.extend(sync.peers.iter().flat_map(peer_lines));
    let height = u16::try_from(lines.len() + 2)
        .unwrap_or(u16::MAX)
        .min(area.height);
    let width = 44.min(area.width);
    let popup = ratatui::layout::Rect::new(area.x, area.bottom() - height, width, height);
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(lines).block(Block::default().borders(Borders::ALL).title(" Sync ")),
        popup,
    );
    hits.push(popup, crate::hit::Target::Inert);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(line: &Line<'static>) -> String {
        line.spans.iter().map(|s| s.content.as_ref()).collect()
    }

    fn peer(device: &str, lag_ms: i64) -> PeerStatus {
        PeerStatus {
            device: device.into(),
            lag_ms,
            ..PeerStatus::default()
        }
    }

    #[test]
    fn no_peers_reads_as_no_peers() {
        let line = render(&SyncSnapshot::default());
        assert!(text(&line).contains("no peers"));
        assert!(text(&line).contains("0 pending"));
    }

    #[test]
    fn shows_peer_count_and_max_lag() {
        let sync = SyncSnapshot {
            peers: vec![peer("a", 100), peer("b", 900)],
            pending_ops: 3,
        };
        let rendered = text(&render(&sync));
        assert!(rendered.contains("2 peers"));
        assert!(
            rendered.contains("900ms"),
            "shows the worst lag, not the best"
        );
        assert!(rendered.contains("3 pending"));
        assert!(!rendered.contains("stuck"));
    }

    #[test]
    fn zero_pending_ops_is_green_nonzero_is_yellow() {
        let converged = render(&SyncSnapshot {
            peers: vec![peer("a", 0)],
            pending_ops: 0,
        });
        assert_eq!(converged.spans[0].style.fg, Some(Color::Green));

        let pending = render(&SyncSnapshot {
            peers: vec![peer("a", 0)],
            pending_ops: 1,
        });
        assert_eq!(pending.spans[0].style.fg, Some(Color::Yellow));
    }

    /// Task sync-drift line 7: a stuck peer turns the dot red and says so in text, and its rows
    /// name the file; a parked peer says it is parked.
    #[test]
    fn a_stuck_peer_is_red_counted_and_named_and_a_parked_one_says_so() {
        let stuck = PeerStatus {
            stuck: vec!["tasks/a/todo.txt".into()],
            ..peer("01J9K3H5Z7Q8X2M4N6P8R0T2V5", 0)
        };
        let line = render(&SyncSnapshot {
            peers: vec![stuck.clone()],
            pending_ops: 0,
        });
        assert_eq!(line.spans[0].style.fg, Some(Color::Red));
        assert!(text(&line).ends_with("1 stuck"), "{}", text(&line));
        let rows: Vec<String> = peer_lines(&stuck).iter().map(text).collect();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[1], "    stuck on tasks/a/todo.txt");
        let parked = PeerStatus {
            parked: true,
            ..peer("b", 0)
        };
        let rows: Vec<String> = peer_lines(&parked).iter().map(text).collect();
        assert_eq!(
            rows.last().map(String::as_str),
            Some("    parked: no shared key")
        );
    }
}
