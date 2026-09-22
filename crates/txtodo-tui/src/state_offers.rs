//! The `o` workspace-offers pane's state (task `workspace-offer-cli`): pending offers from paired
//! peers, the pane's cursor, and the directory draft an accept composes. UI-local mirror of
//! `pb::PendingWorkspaceOffer`, same idiom as `state::ConflictItem`, so `ui/offers.rs` stays
//! proto-free and fixture-testable. Its own file only for `state.rs`'s line budget.

/// One workspace a paired peer offered this device, not yet accepted or declined.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OfferItem {
    /// The offering device's id (ULID text).
    pub device: String,
    /// The offered workspace's id (ULID text); accepting adopts it verbatim.
    pub workspace_id: String,
    /// Human-readable label only; never identity.
    pub name: String,
}

/// Everything the `o` pane renders from and mutates without the daemon.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OffersPane {
    /// Whether the pane is open.
    pub open: bool,
    /// Pending offers, in the daemon's own order; refreshed on `app.rs`'s status tick.
    pub items: Vec<OfferItem>,
    /// Selected index into `items` while the pane is open.
    pub cursor: usize,
    /// The local directory being typed for an `a` accept; `None` when not composing one.
    pub dir_draft: Option<String>,
}

impl OffersPane {
    /// `o`: toggles the pane; resets the cursor and drops any half-typed directory on open.
    pub fn toggle(&mut self) {
        self.open = !self.open;
        self.cursor = 0;
        self.dir_draft = None;
    }

    /// Replaces the offer list, keeping the cursor on a row that still exists.
    pub fn replace(&mut self, items: Vec<OfferItem>) {
        self.items = items;
        self.cursor = self.cursor.min(self.items.len().saturating_sub(1));
    }

    /// The selected offer, if any.
    pub fn selected(&self) -> Option<&OfferItem> {
        self.items.get(self.cursor)
    }

    /// `j`: one row down, clamped to the list.
    pub fn move_down(&mut self) {
        if !self.items.is_empty() {
            self.cursor = (self.cursor + 1).min(self.items.len() - 1);
        }
    }

    /// `k`: one row up, clamped at the first row.
    pub fn move_up(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
    }

    /// `a`: starts composing the directory for the selected offer; a no-op with nothing selected.
    pub fn start_accept(&mut self) {
        if self.selected().is_some() {
            self.dir_draft = Some(String::new());
        }
    }

    /// `Esc` while composing: drops the directory draft, keeps the pane open.
    pub fn cancel_accept(&mut self) {
        self.dir_draft = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn two() -> OffersPane {
        let mut pane = OffersPane::default();
        pane.replace(vec![
            OfferItem {
                device: "d1".into(),
                workspace_id: "w1".into(),
                name: "work".into(),
            },
            OfferItem {
                device: "d2".into(),
                workspace_id: "w2".into(),
                name: "home".into(),
            },
        ]);
        pane
    }

    #[test]
    fn navigation_clamps_and_replace_keeps_the_cursor_in_range() {
        let mut pane = two();
        pane.move_up();
        assert_eq!(pane.cursor, 0);
        pane.move_down();
        pane.move_down();
        assert_eq!(pane.cursor, 1, "clamped at the last row");
        pane.replace(vec![]);
        assert_eq!(pane.cursor, 0);
        assert!(pane.selected().is_none());
    }

    #[test]
    fn accept_draft_needs_a_selection_and_toggle_drops_it() {
        let mut pane = OffersPane::default();
        pane.start_accept();
        assert!(pane.dir_draft.is_none(), "nothing to accept");
        let mut pane = two();
        pane.open = true;
        pane.start_accept();
        assert_eq!(pane.dir_draft.as_deref(), Some(""));
        pane.toggle();
        assert!(!pane.open && pane.dir_draft.is_none());
    }
}
