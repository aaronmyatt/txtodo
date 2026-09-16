//! Session: the happy path end to end, every out-of-order message refused, the skew guard on the
//! link-level `Hello`, and Ack carrying committed runs only — all driven through one opened
//! workspace on the multi-workspace `Session` container. `on_ops` also verifies signatures now
//! (`sync-reject-tests`); these tests pass empty ops/signatures and an empty key map throughout —
//! `sign_tests.rs`/`sealed_ops_tests.rs` own the crypto behaviour itself. Multi-workspace
//! interleaving and the unopened/unknown-workspace error path are `session_multiplex_tests.rs`'s
//! own job, not repeated here.
//!
//! Stage 2: the link-level handshake (`link_hello`/`on_link_hello`, real `Message::Hello`) and a
//! workspace's own greeting (`hello`/`on_hello`, `Message::Greet`) are two separate steps now —
//! `greeted()` below does both, in the order a real caller must (link first).

use std::collections::BTreeMap;

use crate::frame::PROTOCOL_VERSION;
use crate::message::{GroupId, Heads, Message, OriginRange};
use crate::session::{Session, SessionState};
use crate::session_error::SessionError;
use crate::sign::DevicePublicKey;
use crate::want::Gap;
use txtodo_model::{DeviceId, MAX_PEER_SKEW_AHEAD_MS, Skew, Ulid};
use txtodo_store::WorkspaceId;

/// No device keys known; every test here carries zero ops, so `verify_batch` trivially passes
/// regardless of what this map holds.
fn no_keys() -> BTreeMap<DeviceId, DevicePublicKey> {
    BTreeMap::new()
}

const NOW_MS: u64 = 1_700_000_000_000;

fn dev(n: u128) -> DeviceId {
    DeviceId::new(Ulid::from_u128(n))
}

/// The one workspace every test in this file opens.
fn ws() -> WorkspaceId {
    WorkspaceId::new(Ulid::from_u128(1))
}

fn ws_bits() -> u128 {
    ws().ulid().to_u128()
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

/// The peer's link-level `Hello` — device+group+protocol+wall clock, no heads (stage 2: heads
/// moved to `Greet`).
fn peer_link_hello(wall_ms: u64) -> Message {
    Message::Hello {
        device: dev(2),
        group: GroupId(7),
        heads: Heads::new(),
        protocol: PROTOCOL_VERSION,
        wall_ms,
    }
}

/// The peer's `Greet` for `ws()`.
fn peer_greet(heads: Heads) -> Message {
    Message::Greet {
        workspace: ws_bits(),
        heads,
    }
}

/// A session with `ws()` open, holding device 1 up to 5, whose link handshake is done and whose
/// own workspace `Greet` was already sent.
fn greeted() -> Session {
    let mut s = Session::new(dev(1), GroupId(7));
    s.open_workspace(ws(), heads(&[(1, 5)])).unwrap();
    s.link_hello(NOW_MS).unwrap();
    let skew = s
        .on_link_hello(&peer_link_hello(NOW_MS), NOW_MS)
        .unwrap();
    assert_eq!(skew, Skew::Ok);
    let greet = s.hello(ws()).unwrap();
    assert_eq!(
        greet,
        Message::Greet {
            workspace: ws_bits(),
            heads: heads(&[(1, 5)])
        }
    );
    assert_eq!(s.state(ws()).unwrap(), SessionState::Greeted);
    s
}

#[test]
fn happy_path_hello_want_ops_ack_in_two_batches() {
    let mut s = greeted();
    let want = s
        .on_hello(ws(), &peer_greet(heads(&[(1, 5), (2, 4)])))
        .unwrap();
    assert_eq!(
        want,
        Message::Want {
            workspace: ws_bits(),
            ranges: vec![range(2, 1, 4)]
        }
    );
    assert_eq!(s.state(ws()).unwrap(), SessionState::Wanting);
    let first = Message::Ops {
        workspace: ws_bits(),
        ops: Vec::new(),
        signatures: Vec::new(),
        ranges: vec![range(2, 1, 2)],
    };
    assert!(s.on_ops(ws(), &first, &no_keys()).unwrap().is_empty());
    assert_eq!(s.state(ws()).unwrap(), SessionState::Importing);
    let ack = s.committed(ws(), &[range(2, 1, 2)]).unwrap();
    assert_eq!(
        ack,
        Message::Ack {
            workspace: ws_bits(),
            committed: vec![range(2, 1, 2)]
        }
    );
    assert_eq!(
        s.wanted(ws()).unwrap(),
        &[range(2, 3, 4)],
        "the rest is still wanted"
    );
    assert_eq!(s.state(ws()).unwrap(), SessionState::Wanting);
}

#[test]
fn the_second_batch_drains_the_want_and_returns_to_idle() {
    let mut s = greeted();
    s.on_hello(ws(), &peer_greet(heads(&[(1, 5), (2, 4)])))
        .unwrap();
    s.on_ops(
        ws(),
        &Message::Ops {
            workspace: ws_bits(),
            ops: Vec::new(),
            signatures: Vec::new(),
            ranges: vec![range(2, 1, 2)],
        },
        &no_keys(),
    )
    .unwrap();
    s.committed(ws(), &[range(2, 1, 2)]).unwrap();
    let second = Message::Ops {
        workspace: ws_bits(),
        ops: Vec::new(),
        signatures: Vec::new(),
        ranges: vec![range(2, 3, 4)],
    };
    s.on_ops(ws(), &second, &no_keys()).unwrap();
    s.committed(ws(), &[range(2, 3, 4)]).unwrap();
    assert_eq!(
        s.state(ws()).unwrap(),
        SessionState::Idle,
        "nothing left: back to Idle"
    );
    assert_eq!(s.heads(ws()).unwrap(), &heads(&[(1, 5), (2, 4)]));
}

#[test]
fn a_peer_with_nothing_new_leaves_us_idle_with_an_empty_want() {
    let mut s = greeted();
    let want = s.on_hello(ws(), &peer_greet(heads(&[(1, 3)]))).unwrap();
    assert_eq!(
        want,
        Message::Want {
            workspace: ws_bits(),
            ranges: Vec::new()
        }
    );
    assert_eq!(s.state(ws()).unwrap(), SessionState::Idle);
}

#[test]
fn every_message_out_of_order_is_refused_and_changes_nothing() {
    let mut s = Session::new(dev(1), GroupId(7));
    s.open_workspace(ws(), heads(&[])).unwrap();
    let ops = Message::Ops {
        workspace: ws_bits(),
        ops: Vec::new(),
        signatures: Vec::new(),
        ranges: Vec::new(),
    };
    assert_eq!(
        s.on_ops(ws(), &ops, &no_keys()),
        Err(SessionError::Unexpected {
            state: SessionState::Idle,
            what: "Ops"
        })
    );
    assert_eq!(
        s.on_hello(ws(), &peer_greet(heads(&[]))),
        Err(SessionError::LinkNotReady),
        "a workspace cannot be greeted before the link handshake completes"
    );
    assert_eq!(
        s.committed(ws(), &[]),
        Err(SessionError::Unexpected {
            state: SessionState::Idle,
            what: "committed()"
        })
    );
    s.link_hello(NOW_MS).unwrap();
    s.on_link_hello(&peer_link_hello(NOW_MS), NOW_MS).unwrap();
    s.hello(ws()).unwrap();
    assert!(matches!(
        s.hello(ws()),
        Err(SessionError::Unexpected {
            state: SessionState::Greeted,
            ..
        })
    ));
    assert!(matches!(
        s.on_hello(ws(), &ops),
        Err(SessionError::Unexpected { what: "Ops", .. })
    ));
    assert_eq!(s.state(ws()).unwrap(), SessionState::Greeted);
}

#[test]
fn link_hello_checks_group_protocol_and_clock_before_anything_wants_anything() {
    let mut s = Session::new(dev(1), GroupId(7));
    s.link_hello(NOW_MS).unwrap();
    let other_group = Message::Hello {
        device: dev(2),
        group: GroupId(8),
        heads: Heads::new(),
        protocol: PROTOCOL_VERSION,
        wall_ms: NOW_MS,
    };
    assert_eq!(
        s.on_link_hello(&other_group, NOW_MS),
        Err(SessionError::GroupMismatch {
            ours: GroupId(7),
            theirs: GroupId(8)
        })
    );
    let other_protocol = Message::Hello {
        device: dev(2),
        group: GroupId(7),
        heads: Heads::new(),
        protocol: PROTOCOL_VERSION + 1,
        wall_ms: NOW_MS,
    };
    assert_eq!(
        s.on_link_hello(&other_protocol, NOW_MS),
        Err(SessionError::ProtocolMismatch {
            ours: PROTOCOL_VERSION,
            theirs: PROTOCOL_VERSION + 1
        })
    );
    let ahead = NOW_MS + MAX_PEER_SKEW_AHEAD_MS + 1;
    assert_eq!(
        s.on_link_hello(&peer_link_hello(ahead), NOW_MS),
        Err(SessionError::PeerAhead {
            peer_ms: ahead,
            local_ms: NOW_MS,
            lead_ms: MAX_PEER_SKEW_AHEAD_MS + 1
        })
    );
    assert!(s.peer().is_none(), "still waiting for a good Hello");
    let day_ms = 24 * 60 * 60 * 1_000;
    let skew = s
        .on_link_hello(&peer_link_hello(NOW_MS - day_ms), NOW_MS)
        .unwrap();
    assert_eq!(skew, Skew::Behind(day_ms), "behind merges, with a warning");
    assert_eq!(s.peer(), Some(dev(2)));
}

#[test]
fn ops_outside_the_want_and_commits_outside_the_batch_are_refused() {
    let mut s = greeted();
    s.on_hello(ws(), &peer_greet(heads(&[(2, 4)]))).unwrap();
    let stray = Message::Ops {
        workspace: ws_bits(),
        ops: Vec::new(),
        signatures: Vec::new(),
        ranges: vec![range(2, 1, 2), range(3, 1, 1)],
    };
    assert_eq!(
        s.on_ops(ws(), &stray, &no_keys()),
        Err(SessionError::Unrequested(range(3, 1, 1)))
    );
    assert_eq!(s.state(ws()).unwrap(), SessionState::Wanting);
    let batch = Message::Ops {
        workspace: ws_bits(),
        ops: Vec::new(),
        signatures: Vec::new(),
        ranges: vec![range(2, 1, 2)],
    };
    s.on_ops(ws(), &batch, &no_keys()).unwrap();
    assert_eq!(
        s.committed(ws(), &[range(2, 1, 4)]),
        Err(SessionError::NotInBatch(range(2, 1, 4))),
        "cannot ack more than the batch carried"
    );
    assert_eq!(
        s.committed(ws(), &[range(2, 2, 2)]),
        Err(SessionError::Gap(Gap {
            device_head: 0,
            range: range(2, 2, 2)
        })),
        "a partial commit that skips the head is a hole"
    );
    assert_eq!(
        s.state(ws()).unwrap(),
        SessionState::Importing,
        "a refused commit keeps the batch in flight"
    );
    assert_eq!(s.heads(ws()).unwrap(), &heads(&[(1, 5)]), "heads untouched");
    let ack = s.committed(ws(), &[]).unwrap();
    assert_eq!(
        ack,
        Message::Ack {
            workspace: ws_bits(),
            committed: Vec::new()
        },
        "a crash-then-nothing-committed acks nothing, so the peer resends"
    );
    assert_eq!(s.wanted(ws()).unwrap(), &[range(2, 1, 4)]);
}
