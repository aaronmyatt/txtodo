//! Pushed `Ops` (task `sync-live-push`, decided 2026-09-23): once the peer's `Greet` for a
//! workspace was consumed, a batch it sends unasked is accepted in `Idle`, imported and `Ack`ed
//! like a wanted one — but only when it follows the heads we hold, run after run. Empty ops and
//! signatures throughout, like `session_tests.rs`: `sign_tests.rs` owns the crypto itself.

use std::collections::BTreeMap;

use crate::frame::PROTOCOL_VERSION;
use crate::message::{GroupId, Heads, Message, OriginRange};
use crate::session::{Session, SessionState};
use crate::session_error::SessionError;
use crate::want::Gap;
use txtodo_model::{DeviceId, Ulid};
use txtodo_store::WorkspaceId;

const NOW_MS: u64 = 1_700_000_000_000;

fn dev(n: u128) -> DeviceId {
    DeviceId::new(Ulid::from_u128(n))
}

fn ws() -> WorkspaceId {
    WorkspaceId::new(Ulid::from_u128(1))
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

fn ops(ranges: Vec<OriginRange>) -> Message {
    Message::Ops {
        workspace: ws().ulid().to_u128(),
        ops: Vec::new(),
        signatures: Vec::new(),
        ranges,
    }
}

/// A session holding device 2 up to 4, link handshake done, our `Greet` sent.
fn linked() -> Session {
    let mut s = Session::new(dev(1), GroupId(7));
    s.open_workspace(ws(), heads(&[(2, 4)])).unwrap();
    s.link_hello(NOW_MS).unwrap();
    let peer_hello = Message::Hello {
        device: dev(2),
        group: GroupId(7),
        heads: Heads::new(),
        protocol: PROTOCOL_VERSION,
        wall_ms: NOW_MS,
    };
    s.on_link_hello(&peer_hello, NOW_MS).unwrap();
    s.hello(ws()).unwrap();
    s
}

/// `linked()` plus the peer's `Greet` with nothing new: the exchange settles in `Idle`.
fn settled() -> Session {
    let mut s = linked();
    let greet = Message::Greet {
        workspace: ws().ulid().to_u128(),
        heads: heads(&[(2, 4)]),
    };
    s.on_hello(ws(), &greet).unwrap();
    assert_eq!(s.state(ws()).unwrap(), SessionState::Idle);
    s
}

#[test]
fn a_pushed_batch_after_the_greet_is_imported_and_acked() {
    let mut s = settled();
    let push = ops(vec![range(2, 5, 6)]);
    assert!(s.on_ops(ws(), &push, &BTreeMap::new()).unwrap().is_empty());
    assert_eq!(s.state(ws()).unwrap(), SessionState::Importing);

    let ack = s.committed(ws(), &[range(2, 5, 6)]).unwrap();
    assert_eq!(
        ack,
        Message::Ack {
            workspace: ws().ulid().to_u128(),
            committed: vec![range(2, 5, 6)],
        }
    );
    assert_eq!(s.state(ws()).unwrap(), SessionState::Idle);
    assert_eq!(s.heads(ws()).unwrap(), &heads(&[(2, 6)]));

    // The next push follows on, and a new device starts at 1.
    let next = ops(vec![range(2, 7, 7), range(3, 1, 2)]);
    s.on_ops(ws(), &next, &BTreeMap::new()).unwrap();
    s.committed(ws(), &[range(2, 7, 7), range(3, 1, 2)])
        .unwrap();
    assert_eq!(s.heads(ws()).unwrap(), &heads(&[(2, 7), (3, 2)]));
}

#[test]
fn a_push_before_the_peers_greet_is_still_refused() {
    let mut s = linked();
    assert_eq!(
        s.on_ops(ws(), &ops(vec![range(2, 5, 5)]), &BTreeMap::new()),
        Err(SessionError::Unexpected {
            state: SessionState::Greeted,
            what: "Ops"
        })
    );
}

#[test]
fn a_push_with_a_gap_or_a_repeat_is_refused_and_changes_nothing() {
    let mut s = settled();
    assert_eq!(
        s.on_ops(ws(), &ops(vec![range(2, 6, 6)]), &BTreeMap::new()),
        Err(SessionError::Gap(Gap {
            device_head: 4,
            range: range(2, 6, 6),
        })),
        "5 is missing"
    );
    assert!(matches!(
        s.on_ops(ws(), &ops(vec![range(2, 4, 5)]), &BTreeMap::new()),
        Err(SessionError::Gap(_))
    ));
    assert_eq!(s.state(ws()).unwrap(), SessionState::Idle);
    assert_eq!(s.heads(ws()).unwrap(), &heads(&[(2, 4)]));
}

#[test]
fn a_push_while_a_want_is_open_must_still_follow_the_heads() {
    let mut s = linked();
    let greet = Message::Greet {
        workspace: ws().ulid().to_u128(),
        heads: heads(&[(2, 6)]),
    };
    s.on_hello(ws(), &greet).unwrap();
    assert_eq!(s.state(ws()).unwrap(), SessionState::Wanting);
    assert_eq!(
        s.on_ops(ws(), &ops(vec![range(2, 7, 7)]), &BTreeMap::new()),
        Err(SessionError::Gap(Gap {
            device_head: 4,
            range: range(2, 7, 7),
        })),
        "5 and 6 are still missing"
    );
    assert_eq!(s.state(ws()).unwrap(), SessionState::Wanting);
    // A batch reaching past the Want is fine once it follows the heads (task
    // sync-ack-before-held): the sender re-serves from its last acked head, and its heads may
    // have moved on since the Greet.
    s.on_ops(ws(), &ops(vec![range(2, 5, 7)]), &BTreeMap::new())
        .unwrap();
    s.committed(ws(), &[range(2, 5, 7)]).unwrap();
    assert_eq!(s.heads(ws()).unwrap(), &heads(&[(2, 7)]));
    assert_eq!(
        s.state(ws()).unwrap(),
        SessionState::Idle,
        "the Want is met"
    );
}

#[test]
fn a_wanted_batch_after_a_failed_one_is_refused_so_no_hole_is_committed() {
    let mut s = linked();
    let greet = Message::Greet {
        workspace: ws().ulid().to_u128(),
        heads: heads(&[(2, 10)]),
    };
    s.on_hello(ws(), &greet).unwrap();
    s.on_ops(ws(), &ops(vec![range(2, 5, 7)]), &BTreeMap::new())
        .unwrap();
    s.committed(ws(), &[]).unwrap();
    assert_eq!(s.state(ws()).unwrap(), SessionState::Wanting);
    assert_eq!(
        s.on_ops(ws(), &ops(vec![range(2, 8, 10)]), &BTreeMap::new()),
        Err(SessionError::Gap(Gap {
            device_head: 4,
            range: range(2, 8, 10),
        })),
        "5..=7 never landed"
    );
    assert_eq!(s.wanted(ws()).unwrap(), &[range(2, 5, 10)]);
}

#[test]
fn a_partial_commit_acks_the_prefix_and_the_rest_is_wanted_again() {
    let mut s = linked();
    let greet = Message::Greet {
        workspace: ws().ulid().to_u128(),
        heads: heads(&[(2, 10)]),
    };
    s.on_hello(ws(), &greet).unwrap();
    s.on_ops(ws(), &ops(vec![range(2, 5, 10)]), &BTreeMap::new())
        .unwrap();
    let ack = s.committed(ws(), &[range(2, 5, 6)]).unwrap();
    assert_eq!(
        ack,
        Message::Ack {
            workspace: ws().ulid().to_u128(),
            committed: vec![range(2, 5, 6)],
        }
    );
    assert_eq!(s.heads(ws()).unwrap(), &heads(&[(2, 6)]));
    assert_eq!(s.wanted(ws()).unwrap(), &[range(2, 7, 10)]);
    s.on_ops(ws(), &ops(vec![range(2, 7, 10)]), &BTreeMap::new())
        .unwrap();
}
