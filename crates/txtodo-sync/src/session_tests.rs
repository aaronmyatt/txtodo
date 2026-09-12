//! Session: the happy path end to end, every out-of-order message refused, the skew guard on
//! Hello, and Ack carrying committed runs only.

use crate::frame::PROTOCOL_VERSION;
use crate::message::{GroupId, Heads, Message, OriginRange};
use crate::session::{Session, SessionError, SessionState};
use crate::want::Gap;
use txtodo_model::{DeviceId, MAX_PEER_SKEW_AHEAD_MS, Skew, Ulid};

const NOW_MS: u64 = 1_700_000_000_000;

fn dev(n: u128) -> DeviceId {
    DeviceId::new(Ulid::from_u128(n))
}

fn heads(pairs: &[(u128, u64)]) -> Heads {
    pairs.iter().map(|&(d, h)| (dev(d), h)).collect()
}

fn range(device: u128, first: u64, last: u64) -> OriginRange {
    OriginRange {
        device: dev(device),
        first,
        last,
    }
}

fn peer_hello(heads: Heads, wall_ms: u64) -> Message {
    Message::Hello {
        device: dev(2),
        group: GroupId(7),
        heads,
        protocol: PROTOCOL_VERSION,
        wall_ms,
    }
}

/// A session that has said Hello and holds device 1 up to 5.
fn greeted() -> Session {
    let mut s = Session::new(dev(1), GroupId(7), heads(&[(1, 5)]));
    let hello = s.hello(NOW_MS).unwrap();
    assert!(matches!(
        hello,
        Message::Hello {
            wall_ms: NOW_MS,
            ..
        }
    ));
    assert_eq!(s.state(), SessionState::Greeted);
    s
}

#[test]
fn happy_path_hello_want_ops_ack_in_two_batches() {
    let mut s = greeted();
    let g = s
        .on_hello(&peer_hello(heads(&[(1, 5), (2, 4)]), NOW_MS), NOW_MS)
        .unwrap();
    assert_eq!(g.skew, Skew::Ok);
    assert_eq!(
        g.want,
        Message::Want {
            ranges: vec![range(2, 1, 4)]
        }
    );
    assert_eq!(s.state(), SessionState::Wanting);
    let first = Message::Ops {
        ops: Vec::new(),
        ranges: vec![range(2, 1, 2)],
    };
    assert!(s.on_ops(&first).unwrap().is_empty());
    assert_eq!(s.state(), SessionState::Importing);
    let ack = s.committed(&[range(2, 1, 2)]).unwrap();
    assert_eq!(
        ack,
        Message::Ack {
            committed: vec![range(2, 1, 2)]
        }
    );
    assert_eq!(s.wanted(), &[range(2, 3, 4)], "the rest is still wanted");
    assert_eq!(s.state(), SessionState::Wanting);
}

#[test]
fn the_second_batch_drains_the_want_and_returns_to_idle() {
    let mut s = greeted();
    s.on_hello(&peer_hello(heads(&[(1, 5), (2, 4)]), NOW_MS), NOW_MS)
        .unwrap();
    s.on_ops(&Message::Ops {
        ops: Vec::new(),
        ranges: vec![range(2, 1, 2)],
    })
    .unwrap();
    s.committed(&[range(2, 1, 2)]).unwrap();
    let second = Message::Ops {
        ops: Vec::new(),
        ranges: vec![range(2, 3, 4)],
    };
    s.on_ops(&second).unwrap();
    s.committed(&[range(2, 3, 4)]).unwrap();
    assert_eq!(s.state(), SessionState::Idle, "nothing left: back to Idle");
    assert_eq!(s.heads(), &heads(&[(1, 5), (2, 4)]));
}

#[test]
fn a_peer_with_nothing_new_leaves_us_idle_with_an_empty_want() {
    let mut s = greeted();
    let g = s
        .on_hello(&peer_hello(heads(&[(1, 3)]), NOW_MS), NOW_MS)
        .unwrap();
    assert_eq!(g.want, Message::Want { ranges: Vec::new() });
    assert_eq!(s.state(), SessionState::Idle);
}

#[test]
fn every_message_out_of_order_is_refused_and_changes_nothing() {
    let mut s = Session::new(dev(1), GroupId(7), heads(&[]));
    let ops = Message::Ops {
        ops: Vec::new(),
        ranges: Vec::new(),
    };
    assert_eq!(
        s.on_ops(&ops),
        Err(SessionError::Unexpected {
            state: SessionState::Idle,
            what: "Ops"
        })
    );
    assert_eq!(
        s.on_hello(&peer_hello(heads(&[]), NOW_MS), NOW_MS),
        Err(SessionError::Unexpected {
            state: SessionState::Idle,
            what: "Hello"
        })
    );
    assert_eq!(
        s.committed(&[]),
        Err(SessionError::Unexpected {
            state: SessionState::Idle,
            what: "committed()"
        })
    );
    s.hello(NOW_MS).unwrap();
    assert!(matches!(
        s.hello(NOW_MS),
        Err(SessionError::Unexpected {
            state: SessionState::Greeted,
            ..
        })
    ));
    assert!(matches!(
        s.on_hello(&ops, NOW_MS),
        Err(SessionError::Unexpected { what: "Ops", .. })
    ));
    assert_eq!(s.state(), SessionState::Greeted);
}

#[test]
fn hello_checks_group_protocol_and_clock_before_wanting_anything() {
    let mut s = greeted();
    let other_group = Message::Hello {
        device: dev(2),
        group: GroupId(8),
        heads: heads(&[(2, 1)]),
        protocol: PROTOCOL_VERSION,
        wall_ms: NOW_MS,
    };
    assert_eq!(
        s.on_hello(&other_group, NOW_MS),
        Err(SessionError::GroupMismatch {
            ours: GroupId(7),
            theirs: GroupId(8)
        })
    );
    let other_protocol = Message::Hello {
        device: dev(2),
        group: GroupId(7),
        heads: heads(&[(2, 1)]),
        protocol: PROTOCOL_VERSION + 1,
        wall_ms: NOW_MS,
    };
    assert_eq!(
        s.on_hello(&other_protocol, NOW_MS),
        Err(SessionError::ProtocolMismatch { ours: 1, theirs: 2 })
    );
    let ahead = NOW_MS + MAX_PEER_SKEW_AHEAD_MS + 1;
    assert_eq!(
        s.on_hello(&peer_hello(heads(&[(2, 1)]), ahead), NOW_MS),
        Err(SessionError::PeerAhead {
            peer_ms: ahead,
            local_ms: NOW_MS,
            lead_ms: MAX_PEER_SKEW_AHEAD_MS + 1
        })
    );
    assert_eq!(
        s.state(),
        SessionState::Greeted,
        "still waiting for a good Hello"
    );
    assert!(s.wanted().is_empty());
    let day_ms = 24 * 60 * 60 * 1_000;
    let g = s
        .on_hello(&peer_hello(heads(&[(2, 1)]), NOW_MS - day_ms), NOW_MS)
        .unwrap();
    assert_eq!(
        g.skew,
        Skew::Behind(day_ms),
        "behind merges, with a warning"
    );
    assert_eq!(s.state(), SessionState::Wanting);
}

#[test]
fn ops_outside_the_want_and_commits_outside_the_batch_are_refused() {
    let mut s = greeted();
    s.on_hello(&peer_hello(heads(&[(2, 4)]), NOW_MS), NOW_MS)
        .unwrap();
    let stray = Message::Ops {
        ops: Vec::new(),
        ranges: vec![range(2, 1, 2), range(3, 1, 1)],
    };
    assert_eq!(
        s.on_ops(&stray),
        Err(SessionError::Unrequested(range(3, 1, 1)))
    );
    assert_eq!(s.state(), SessionState::Wanting);
    let batch = Message::Ops {
        ops: Vec::new(),
        ranges: vec![range(2, 1, 2)],
    };
    s.on_ops(&batch).unwrap();
    assert_eq!(
        s.committed(&[range(2, 1, 4)]),
        Err(SessionError::NotInBatch(range(2, 1, 4))),
        "cannot ack more than the batch carried"
    );
    assert_eq!(
        s.committed(&[range(2, 2, 2)]),
        Err(SessionError::Gap(Gap {
            device_head: 0,
            range: range(2, 2, 2)
        })),
        "a partial commit that skips the head is a hole"
    );
    assert_eq!(
        s.state(),
        SessionState::Importing,
        "a refused commit keeps the batch in flight"
    );
    assert_eq!(s.heads(), &heads(&[(1, 5)]), "heads untouched");
    let ack = s.committed(&[]).unwrap();
    assert_eq!(
        ack,
        Message::Ack {
            committed: Vec::new()
        },
        "a crash-then-nothing-committed acks nothing, so the peer resends"
    );
    assert_eq!(s.wanted(), &[range(2, 1, 4)]);
}
