//! `settings.rs`'s tests: the Devices card with this device's code showing, on a test terminal.

use super::*;
use crate::state_settings::Offer;
use ratatui::Terminal;
use ratatui::backend::TestBackend;

fn offering(qr: Vec<String>) -> AppState {
    let mut state = AppState::fixture();
    state.settings.pairing = Pairing::Offer(Offer {
        code: "CODE42".to_owned(),
        qr,
        until: std::time::Instant::now() + std::time::Duration::from_secs(90),
    });
    state
}

fn drawn(state: &AppState, width: u16, height: u16) -> Vec<String> {
    let mut terminal =
        Terminal::new(TestBackend::new(width, height)).unwrap_or_else(|e| panic!("{e}"));
    let mut hits = HitMap::default();
    terminal
        .draw(|f| draw(f, f.area(), state, SettingsCard::Devices, &mut hits))
        .unwrap_or_else(|e| panic!("{e}"));
    terminal
        .backend()
        .buffer()
        .content()
        .chunks(usize::from(width))
        .map(|r| r.iter().map(|c| c.symbol()).collect())
        .collect()
}

#[test]
fn the_qr_draws_under_the_rows_when_it_fits() {
    let qr = crate::pair_code::qr_lines("txtodo");
    let rows = drawn(&offering(qr.clone()), 80, 40);
    assert!(rows.iter().any(|r| r.contains("CODE42")), "{rows:#?}");
    let first = qr
        .first()
        .map(|l| l.trim_end().to_owned())
        .unwrap_or_default();
    assert!(rows.iter().any(|r| r.contains(&first)), "{rows:#?}");
}

#[test]
fn a_short_pane_says_how_much_room_the_qr_needs() {
    let qr = crate::pair_code::qr_lines("txtodo");
    let rows = drawn(&offering(qr), 80, 12);
    assert!(rows.iter().any(|r| r.contains("The QR needs")), "{rows:#?}");
    assert!(rows.iter().any(|r| r.contains("CODE42")));
}
