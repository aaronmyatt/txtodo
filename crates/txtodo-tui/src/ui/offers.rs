//! The `o` workspace-offers pane (task `workspace-offer-cli`): lists what paired peers offered,
//! `a` composes a local directory and accepts into it, `d` declines. The daemon requires a
//! directory for an accept (no default location until `remote-workspace-mirror` decides one), so
//! `a` opens a one-line prompt rather than adopting somewhere invented.

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState};
use txtodo_proto::v1 as pb;

use crate::action::Action;
use crate::state::AppState;

/// One keystroke while the pane has focus. Returns the [`Action`] to send, if any.
pub fn on_key(state: &mut AppState, key: KeyEvent) -> Option<Action> {
    if state.offers.dir_draft.is_some() {
        return on_draft_key(state, key);
    }
    match key.code {
        KeyCode::Esc | KeyCode::Char('o') => state.offers.toggle(),
        KeyCode::Char('j') | KeyCode::Down => state.offers.move_down(),
        KeyCode::Char('k') | KeyCode::Up => state.offers.move_up(),
        KeyCode::Char('a') => state.offers.start_accept(),
        KeyCode::Char('d') => return decline_request(state).map(Action::DeclineOffer),
        _ => {}
    }
    None
}

/// Typing the directory: `Enter` accepts (a blank directory is a no-op, not an accept into
/// nowhere), `Esc` cancels, the rest edits the buffer.
fn on_draft_key(state: &mut AppState, key: KeyEvent) -> Option<Action> {
    match key.code {
        KeyCode::Esc => state.offers.cancel_accept(),
        KeyCode::Enter => {
            let req = accept_request(state);
            if req.is_some() {
                state.offers.cancel_accept();
            }
            return req.map(Action::AcceptOffer);
        }
        KeyCode::Backspace => {
            state.offers.dir_draft.as_mut()?.pop();
        }
        KeyCode::Char(c) => state.offers.dir_draft.as_mut()?.push(c),
        _ => {}
    }
    None
}

/// The accept request for the selected offer and the typed directory; `None` when either is
/// missing or the directory is blank.
pub fn accept_request(state: &AppState) -> Option<pb::WorkspaceAcceptOfferRequest> {
    let offer = state.offers.selected()?;
    let dir = state.offers.dir_draft.as_deref()?.trim();
    if dir.is_empty() {
        return None;
    }
    Some(pb::WorkspaceAcceptOfferRequest {
        offering_device: offer.device.clone(),
        workspace_id: offer.workspace_id.clone(),
        local_dir: dir.to_owned(),
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

/// Renders the pane over `area`; the directory prompt takes the title while it is open.
pub fn draw(frame: &mut Frame, area: Rect, state: &AppState) {
    let title = match (&state.offers.dir_draft, state.offers.selected()) {
        (Some(dir), Some(offer)) => format!("accept {} into dir> {dir}", label(&offer.name)),
        _ => "workspace offers: a=accept d=decline".to_owned(),
    };
    let block = Block::default().borders(Borders::ALL).title(title);
    let items: Vec<ListItem> = if state.offers.items.is_empty() {
        vec![ListItem::new("no pending workspace offers")]
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
    use crate::state_offers::OfferItem;
    use crossterm::event::KeyModifiers;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
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
    fn a_then_a_typed_directory_then_enter_accepts_and_blank_does_not() {
        let mut state = state_with_offer();
        assert!(on_key(&mut state, key(KeyCode::Char('a'))).is_none());
        assert!(
            on_key(&mut state, key(KeyCode::Enter)).is_none(),
            "blank dir"
        );
        for c in "/tmp/w".chars() {
            on_key(&mut state, key(KeyCode::Char(c)));
        }
        let Some(Action::AcceptOffer(req)) = on_key(&mut state, key(KeyCode::Enter)) else {
            panic!("expected AcceptOffer")
        };
        assert_eq!(req.local_dir, "/tmp/w");
        assert_eq!(req.workspace_id, "w1");
        assert!(state.offers.dir_draft.is_none(), "draft consumed");
    }

    #[test]
    fn esc_cancels_the_draft_before_it_closes_the_pane() {
        let mut state = state_with_offer();
        on_key(&mut state, key(KeyCode::Char('a')));
        on_key(&mut state, key(KeyCode::Esc));
        assert!(state.offers.open && state.offers.dir_draft.is_none());
        on_key(&mut state, key(KeyCode::Esc));
        assert!(!state.offers.open);
    }
}
