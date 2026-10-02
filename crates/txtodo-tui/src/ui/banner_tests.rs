//! `banner.rs`'s tests: which banners a state calls for, and where their buttons land.

use super::*;
use crate::state_shell::Refused;
use ratatui::Terminal;
use ratatui::backend::TestBackend;

#[test]
fn banners_come_most_urgent_first() {
    let mut state = AppState::fixture(); // one flagged line
    assert_eq!(banners(&state)[0].label, "1 line needs review");
    state.shell.link = Link::Connecting;
    state.shell.refused = Some(Refused {
        error: "line 3 changed".to_owned(),
        text: "call mom".to_owned(),
    });
    state.shell.daemon_build = Some(("0.0.9".to_owned(), "2026-09-01".to_owned()));
    state.skill_hint = true;
    let labels: Vec<String> = banners(&state).into_iter().map(|b| b.label).collect();
    assert_eq!(
        labels,
        [
            "daemon: connecting",
            "1 line needs review",
            "Your edit to todo.txt was not saved",
            "txtodod is v0.0.9 \u{b7} 2026-09-01",
            "No agent playbook installed",
        ]
    );
    state.shell.conflict_banner_hidden = true;
    assert!(!banners(&state).iter().any(|b| b.label.contains("review")));
}

#[test]
fn buttons_sit_at_the_right_edge_and_run_their_commands() {
    let mut state = AppState::fixture();
    state.shell.link = Link::Down;
    let rows = banners(&state);
    let mut terminal = Terminal::new(TestBackend::new(80, 2)).unwrap_or_else(|e| panic!("{e}"));
    let mut hits = HitMap::default();
    terminal
        .draw(|f| draw(f, f.area(), &rows, &mut hits))
        .unwrap_or_else(|e| panic!("{e}"));
    let text: String = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|c| c.symbol())
        .collect();
    let (first, second) = text.split_at(80);
    assert!(first.starts_with(" daemon: not answering"), "{first}");
    assert!(first.ends_with(" Retry  "), "{first}");
    assert!(second.contains(" Review   \u{d7}  "), "{second}");
    assert_eq!(
        hits.at(75, 0),
        Some(Target::Command(Command::AppRetryDaemon))
    );
    assert_eq!(
        hits.at(78, 1),
        Some(Target::Command(Command::ConflictsDismissBanner))
    );
    assert_eq!(hits.at(3, 1), Some(Target::Inert));
}

/// Task sync-divergence-check/protocol-mismatch: a peer on another sync protocol gets a loud
/// banner, right after the daemon's own, naming both protocols and which device to upgrade.
#[test]
fn a_peer_on_another_protocol_gets_a_loud_banner_saying_which_device_to_upgrade() {
    let mut state = AppState::fixture();
    assert!(
        !banners(&state)
            .iter()
            .any(|b| b.label.contains("not syncing"))
    );
    state.sync.peers[0].their_protocol = 3;
    let rows = banners(&state);
    let b = rows
        .iter()
        .find(|b| b.label == "A paired device is not syncing")
        .unwrap_or_else(|| panic!("{rows:?}"));
    assert!(b.loud);
    assert_eq!(
        b.detail,
        "01J9K3H5Z7\u{2026} speaks sync protocol 3, this device 2; this device is older: \
         upgrade txtodo here."
    );
    assert_eq!(b.buttons, vec![("Sync", Command::SyncOpen)]);
    state.sync.peers[0].their_protocol = 1;
    let rows = banners(&state);
    assert!(
        rows.iter()
            .any(|b| b.detail.ends_with("upgrade txtodo on that device."))
    );
}
