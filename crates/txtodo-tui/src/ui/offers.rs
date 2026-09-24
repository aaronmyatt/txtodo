//! The `o` workspace-offers pane (task `workspace-offer-cli`): lists what paired peers offered,
//! `a` accepts, `d` declines. No directory prompt (task `remote-workspace-mirror`): the daemon
//! mirrors every offer into its own folder, and on its own too, so the list is usually empty.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState};
use txtodo_proto::v1 as pb;

use crate::state::AppState;

/// The accept request for the selected offer, if any. The daemon picks the folder.
pub fn accept_request(state: &AppState) -> Option<pb::WorkspaceAcceptOfferRequest> {
    let offer = state.offers.selected()?;
    Some(pb::WorkspaceAcceptOfferRequest {
        offering_device: offer.device.clone(),
        workspace_id: offer.workspace_id.clone(),
    })
}

/// The decline request for the selected offer, if any.
pub fn decline_request(state: &AppState) -> Option<pb::WorkspaceDeclineOfferRequest> {
    let offer = state.offers.selected()?;
    Some(pb::WorkspaceDeclineOfferRequest {
        offering_device: offer.device.clone(),
        workspace_id: offer.workspace_id.clone(),
    })
}

/// Renders the pane over `area`.
pub fn draw(frame: &mut Frame, area: Rect, state: &AppState) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title("workspace offers: a=accept d=decline");
    let items: Vec<ListItem> = if state.offers.items.is_empty() {
        vec![ListItem::new(empty_text(&state.offers.problem))]
    } else {
        state
            .offers
            .items
            .iter()
            .map(|o| ListItem::new(row(o)))
            .collect()
    };
    let mut list_state = ListState::default().with_selected(Some(state.offers.cursor));
    let list = List::new(items)
        .block(block)
        .highlight_style(Style::new().fg(Color::Black).bg(Color::Yellow));
    frame.render_stateful_widget(list, area, &mut list_state);
}

/// The empty pane's one line: "blocked" with its reason while the daemon's offer exchange fails.
fn empty_text(problem: &str) -> String {
    if problem.is_empty() {
        "no pending workspace offers".to_owned()
    } else {
        format!("offers blocked: {problem} (see docs/keychain-runbook.md)")
    }
}

fn label(name: &str) -> &str {
    if name.is_empty() { "(unnamed)" } else { name }
}

/// `<name>  <workspace id>  from <device>` — the name first here, unlike the CLI, since a pane
/// row is picked by cursor, never typed.
fn row(o: &crate::state_offers::OfferItem) -> String {
    format!("{}  {}  from {}", label(&o.name), o.workspace_id, o.device)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::action::Action;
    use crate::state_offers::OfferItem;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    /// A key through the real dispatch (`input.rs` -> keymap -> `commands.rs`).
    fn on_key(state: &mut AppState, key: KeyEvent) -> Option<Action> {
        crate::input::Input::default().on_key(state, key)
    }

    fn state_with_offer() -> AppState {
        let mut state = AppState::fixture();
        state.offers.open = true;
        state.offers.replace(vec![OfferItem {
            device: "d1".into(),
            workspace_id: "w1".into(),
            name: "work".into(),
        }]);
        state
    }

    #[test]
    fn d_declines_the_selected_offer() {
        let mut state = state_with_offer();
        let Some(Action::DeclineOffer(req)) = on_key(&mut state, key(KeyCode::Char('d'))) else {
            panic!("expected DeclineOffer")
        };
        assert_eq!(req.offering_device, "d1");
        assert_eq!(req.workspace_id, "w1");
        let mut empty = AppState::fixture();
        empty.offers.open = true;
        assert!(on_key(&mut empty, key(KeyCode::Char('d'))).is_none());
    }

    #[test]
    fn a_accepts_the_selected_offer_at_once_with_no_directory_prompt() {
        let mut state = state_with_offer();
        let Some(Action::AcceptOffer(req)) = on_key(&mut state, key(KeyCode::Char('a'))) else {
            panic!("expected AcceptOffer")
        };
        assert_eq!(req.offering_device, "d1");
        assert_eq!(req.workspace_id, "w1");
        let mut empty = AppState::fixture();
        empty.offers.open = true;
        assert!(on_key(&mut empty, key(KeyCode::Char('a'))).is_none());
    }

    #[test]
    fn an_empty_pane_says_blocked_when_the_offer_channel_fails() {
        assert_eq!(empty_text(""), "no pending workspace offers");
        assert!(empty_text("keystore timed out").starts_with("offers blocked: keystore"));
    }

    #[test]
    fn esc_closes_the_pane() {
        let mut state = state_with_offer();
        on_key(&mut state, key(KeyCode::Esc));
        assert!(!state.offers.open);
    }
}
