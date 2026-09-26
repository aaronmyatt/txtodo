//! `peer_keys.rs` (task sync-drift line 5): when a peer is parked for having no shared key, what
//! brings it back, and which open failures warn.

use txtodo_model::{DeviceId, Ulid};

use crate::peer_keys::{
    MAX_TRACKED_PEERS, Noted, PARK_AFTER, PeerKeys, PeerSignal, SessionEnd, WRONG_GROUP,
};

fn peer(n: u128) -> DeviceId {
    DeviceId::new(Ulid::from_u128(n))
}

fn wrong_group(keys: &PeerKeys, peer: DeviceId, times: u32) {
    for _ in 0..times {
        keys.book(Some(peer), PeerSignal::OpenFailed(WRONG_GROUP), "sync");
    }
}

#[test]
fn a_peer_is_parked_after_park_after_wrong_group_opens_in_a_row() {
    let keys = PeerKeys::default();
    wrong_group(&keys, peer(1), PARK_AFTER - 1);
    assert!(!keys.is_parked(peer(1)), "one short of the limit");
    wrong_group(&keys, peer(1), 1);
    assert!(keys.is_parked(peer(1)));
    assert!(!keys.is_parked(peer(2)), "other peers are untouched");
}

#[test]
fn another_failure_kind_or_an_open_ends_the_run() {
    let keys = PeerKeys::default();
    wrong_group(&keys, peer(1), PARK_AFTER - 1);
    keys.book(
        Some(peer(1)),
        PeerSignal::OpenFailed("unknown_epoch"),
        "control",
    );
    wrong_group(&keys, peer(1), PARK_AFTER - 1);
    assert!(!keys.is_parked(peer(1)), "unknown_epoch broke the run");
    keys.book(Some(peer(1)), PeerSignal::Opened, "sync");
    wrong_group(&keys, peer(1), PARK_AFTER - 1);
    assert!(!keys.is_parked(peer(1)), "an opened frame broke the run");
    keys.book(Some(peer(1)), PeerSignal::Silent, "sync");
    wrong_group(&keys, peer(1), 1);
    assert!(keys.is_parked(peer(1)), "silence is no evidence either way");
}

#[test]
fn a_parked_peer_comes_back_on_forget_open_or_a_group_change() {
    let keys = PeerKeys::default();
    wrong_group(&keys, peer(1), PARK_AFTER);
    keys.forget(peer(1), "paired");
    assert!(!keys.is_parked(peer(1)), "a pairing unparks");

    wrong_group(&keys, peer(1), PARK_AFTER);
    keys.book_session(None, SessionEnd::Greeted(peer(1)));
    assert!(
        !keys.is_parked(peer(1)),
        "its Hello opened on an incoming session"
    );

    wrong_group(&keys, peer(1), PARK_AFTER);
    keys.clear();
    assert!(!keys.is_parked(peer(1)), "our group changed");
}

#[test]
fn the_unknown_peer_of_an_incoming_session_is_never_parked() {
    let keys = PeerKeys::default();
    for _ in 0..PARK_AFTER * 2 {
        keys.book_session(None, SessionEnd::Refused(WRONG_GROUP));
    }
    let noted = keys.note_failure(None, WRONG_GROUP);
    assert!(!noted.parked_now);
    assert!(!noted.first, "the unknown peer warned once already");
}

#[test]
fn a_dialed_session_books_against_the_dialed_peer() {
    let keys = PeerKeys::default();
    for _ in 0..PARK_AFTER {
        keys.book_session(Some(peer(1)), SessionEnd::Refused(WRONG_GROUP));
    }
    assert!(keys.is_parked(peer(1)));
    keys.book_session(Some(peer(1)), SessionEnd::NoHello);
    assert!(
        keys.is_parked(peer(1)),
        "a session with no Hello says nothing"
    );
}

#[test]
fn a_failure_warns_once_per_peer_and_kind_and_parks_once() {
    let keys = PeerKeys::default();
    let first = keys.note_failure(Some(peer(1)), WRONG_GROUP);
    assert_eq!(
        first,
        Noted {
            first: true,
            parked_now: false
        }
    );
    assert!(!keys.note_failure(Some(peer(1)), WRONG_GROUP).first);
    assert!(keys.note_failure(Some(peer(1)), WRONG_GROUP).parked_now);
    assert!(
        !keys.note_failure(Some(peer(1)), WRONG_GROUP).parked_now,
        "parking is logged once, when the run reaches the limit"
    );
    assert!(keys.note_failure(Some(peer(1)), "unknown_epoch").first);
    assert!(keys.note_failure(Some(peer(2)), WRONG_GROUP).first);
    keys.forget(peer(1), "sighted");
    assert!(
        !keys.note_failure(Some(peer(1)), WRONG_GROUP).first,
        "unparking does not re-arm the warning"
    );
}

#[test]
fn peers_past_the_cap_are_not_tracked() {
    let keys = PeerKeys::default();
    for n in 0..MAX_TRACKED_PEERS as u128 {
        keys.note_failure(Some(peer(n + 1)), "decrypt");
    }
    let extra = peer(MAX_TRACKED_PEERS as u128 + 1);
    wrong_group(&keys, extra, PARK_AFTER);
    assert!(!keys.is_parked(extra));
    assert!(!keys.note_failure(Some(extra), WRONG_GROUP).first);
}

#[test]
fn only_a_greeted_session_is_a_successful_dial() {
    assert_eq!(
        SessionEnd::of(Some(peer(1)), None),
        SessionEnd::Greeted(peer(1))
    );
    assert_eq!(
        SessionEnd::of(None, Some(WRONG_GROUP)),
        SessionEnd::Refused(WRONG_GROUP)
    );
    assert_eq!(SessionEnd::of(None, None), SessionEnd::NoHello);
    assert!(SessionEnd::Greeted(peer(1)).greeted());
    assert!(!SessionEnd::Refused(WRONG_GROUP).greeted());
    assert!(!SessionEnd::NoHello.greeted());
}
