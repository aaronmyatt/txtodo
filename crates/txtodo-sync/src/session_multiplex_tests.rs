//! The actual point of task `daemon-workspace-session-multiplex`: two (or more) workspaces'
//! Want/Ack bookkeeping genuinely interleaves on one `Session`, without either workspace's own
//! state machine ever seeing the other's messages — plus the typed errors a caller gets for an
//! unopened/unknown workspace, a message routed to the wrong one, and the open-workspace cap.
//! `session_tests.rs` owns the single-workspace state-machine behaviour itself; this file only
//! covers what genuinely changed by giving `Session` a workspace dimension.
//!
//! Stage 2: every workspace-level `Greet` now requires the link-level `Hello` handshake
//! (`do_link_handshake`) to have completed first — a real, deliberate precondition
//! (`SessionError::LinkNotReady`), not an oversight; `an_operation_against_an_unopened_workspace_
//! is_a_typed_error_not_a_panic` below still expects `UnknownWorkspace` (not `LinkNotReady`) for a
//! workspace this session never opened at all, because `Session::on_hello` checks "is this
//! workspace even open" before "is the link ready" — the more specific, actionable error wins.

use std::collections::BTreeMap;

use crate::frame::PROTOCOL_VERSION;
use crate::message::{GroupId, Heads, Message, OriginRange};
use crate::session::{MAX_OPEN_WORKSPACES, Session, SessionState};
use crate::session_error::SessionError;
use txtodo_model::{DeviceId, Ulid};
use txtodo_store::WorkspaceId;

const NOW_MS: u64 = 1_700_000_000_000;

fn dev(n: u128) -> DeviceId {
    DeviceId::new(Ulid::from_u128(n))
}

fn wsid(n: u128) -> WorkspaceId {
    WorkspaceId::new(Ulid::from_u128(n))
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

fn peer_link_hello() -> Message {
    Message::Hello {
        device: dev(9),
        group: GroupId(1),
        heads: Heads::new(),
        protocol: PROTOCOL_VERSION,
        wall_ms: NOW_MS,
    }
}

fn peer_greet(workspace: WorkspaceId, heads: Heads) -> Message {
    Message::Greet {
        workspace: workspace.ulid().to_u128(),
        heads,
    }
}

fn no_keys() -> BTreeMap<DeviceId, crate::sign::DevicePublicKey> {
    BTreeMap::new()
}

/// The one-time link-level handshake every workspace's own `Greet` now requires
/// (`SessionError::LinkNotReady` otherwise).
fn do_link_handshake(s: &mut Session) {
    s.link_hello(NOW_MS).unwrap();
    s.on_link_hello(&peer_link_hello(), NOW_MS).unwrap();
}

/// Two workspaces, opened at different local heads so their `Want`s (and therefore their whole
/// exchange) are distinguishable if bookkeeping ever crossed.
fn two_open_workspaces() -> (Session, WorkspaceId, WorkspaceId) {
    let a = wsid(1);
    let b = wsid(2);
    let mut s = Session::new(dev(1), GroupId(1));
    s.open_workspace(a, heads(&[(9, 2)])).unwrap();
    s.open_workspace(b, heads(&[(9, 5)])).unwrap();
    (s, a, b)
}

#[test]
fn two_workspaces_hello_and_want_interleave_without_crossing() {
    let (mut s, a, b) = two_open_workspaces();
    do_link_handshake(&mut s);
    s.hello(a).unwrap();
    s.hello(b).unwrap();
    assert_eq!(s.state(a).unwrap(), SessionState::Greeted);
    assert_eq!(s.state(b).unwrap(), SessionState::Greeted);

    // Interleaved: b's Greet lands before a's, and each workspace's Want reflects only its own
    // local heads (2 for a, 5 for b) diffed against the same peer heads (10).
    let peer_heads = heads(&[(9, 10)]);
    let want_b = s.on_hello(b, &peer_greet(b, peer_heads.clone())).unwrap();
    let want_a = s.on_hello(a, &peer_greet(a, peer_heads)).unwrap();

    assert_eq!(
        want_a,
        Message::Want {
            workspace: a.ulid().to_u128(),
            ranges: vec![range(9, 3, 10)]
        },
        "a's Want reflects a's own heads (2), not b's (5)"
    );
    assert_eq!(
        want_b,
        Message::Want {
            workspace: b.ulid().to_u128(),
            ranges: vec![range(9, 6, 10)]
        },
        "b's Want reflects b's own heads (5), not a's (2)"
    );
    assert_eq!(s.state(a).unwrap(), SessionState::Wanting);
    assert_eq!(s.state(b).unwrap(), SessionState::Wanting);
}

fn ops_for(workspace: WorkspaceId, r: OriginRange) -> Message {
    Message::Ops {
        workspace: workspace.ulid().to_u128(),
        ops: Vec::new(),
        signatures: Vec::new(),
        ranges: vec![r],
    }
}

/// Greets and hellos both `a` and `b` against the same peer heads, leaving both `Wanting`.
fn greet_both(s: &mut Session, a: WorkspaceId, b: WorkspaceId) {
    do_link_handshake(s);
    s.hello(a).unwrap();
    s.hello(b).unwrap();
    let peer_heads = heads(&[(9, 10)]);
    s.on_hello(a, &peer_greet(a, peer_heads.clone())).unwrap();
    s.on_hello(b, &peer_greet(b, peer_heads)).unwrap();
}

/// Asserts `s.committed(workspace, ...)`'s `Ack` names exactly `r`, and that workspace lands at
/// `Idle` with `r`'s `last` as its new head — split out purely to keep the test itself under this
/// workspace's cognitive-complexity budget (`clippy.toml`).
fn assert_committed_and_idle(s: &mut Session, workspace: WorkspaceId, r: OriginRange) {
    let ack = s.committed(workspace, &[r]).unwrap();
    assert_eq!(
        ack,
        Message::Ack {
            workspace: workspace.ulid().to_u128(),
            committed: vec![r]
        }
    );
    assert_eq!(s.heads(workspace).unwrap(), &heads(&[(9, r.last)]));
    assert_eq!(s.state(workspace).unwrap(), SessionState::Idle);
}

#[test]
fn two_workspaces_ops_and_committed_interleave_without_crossing() {
    let (mut s, a, b) = two_open_workspaces();
    greet_both(&mut s, a, b);

    // b's Ops arrives, then a's — genuinely interleaved, and each workspace's own batch must be
    // accepted independently.
    assert!(
        s.on_ops(b, &ops_for(b, range(9, 6, 10)), &no_keys())
            .unwrap()
            .is_empty()
    );
    assert!(
        s.on_ops(a, &ops_for(a, range(9, 3, 10)), &no_keys())
            .unwrap()
            .is_empty()
    );
    assert_eq!(s.state(a).unwrap(), SessionState::Importing);
    assert_eq!(s.state(b).unwrap(), SessionState::Importing);

    // Then b commits, then a — each workspace's own batch must land only in that workspace's own
    // heads, never the other's.
    assert_committed_and_idle(&mut s, b, range(9, 6, 10));
    assert_committed_and_idle(&mut s, a, range(9, 3, 10));
}

#[test]
fn an_operation_against_an_unopened_workspace_is_a_typed_error_not_a_panic() {
    let mut s = Session::new(dev(1), GroupId(1));
    let never_opened = wsid(404);
    assert_eq!(
        s.hello(never_opened),
        Err(SessionError::UnknownWorkspace(never_opened))
    );
    assert_eq!(
        s.on_hello(never_opened, &peer_greet(never_opened, heads(&[]))),
        Err(SessionError::UnknownWorkspace(never_opened)),
        "an unopened workspace is UnknownWorkspace even before the link handshake completes"
    );
    let ops = Message::Ops {
        workspace: never_opened.ulid().to_u128(),
        ops: Vec::new(),
        signatures: Vec::new(),
        ranges: Vec::new(),
    };
    assert_eq!(
        s.on_ops(never_opened, &ops, &no_keys()),
        Err(SessionError::UnknownWorkspace(never_opened))
    );
    assert_eq!(
        s.committed(never_opened, &[]),
        Err(SessionError::UnknownWorkspace(never_opened))
    );
    assert_eq!(
        s.state(never_opened),
        Err(SessionError::UnknownWorkspace(never_opened))
    );
    assert_eq!(
        s.heads(never_opened),
        Err(SessionError::UnknownWorkspace(never_opened))
    );
    assert_eq!(
        s.wanted(never_opened),
        Err(SessionError::UnknownWorkspace(never_opened))
    );
    assert!(!s.is_open(never_opened));
}

#[test]
fn a_workspace_cannot_be_greeted_before_the_link_handshake_completes() {
    let (mut s, a, _b) = two_open_workspaces();
    // No `do_link_handshake` call at all: `a` is genuinely open, but the link never greeted.
    assert_eq!(
        s.on_hello(a, &peer_greet(a, heads(&[(9, 10)]))),
        Err(SessionError::LinkNotReady)
    );
    assert_eq!(
        s.state(a).unwrap(),
        SessionState::Idle,
        "a refused Greet changes nothing"
    );
}

#[test]
fn an_ops_message_tagged_for_a_different_workspace_is_refused_not_misrouted() {
    let (mut s, a, b) = two_open_workspaces();
    do_link_handshake(&mut s);
    s.hello(a).unwrap();
    let peer_heads = heads(&[(9, 10)]);
    s.on_hello(a, &peer_greet(a, peer_heads)).unwrap();

    // A message that carries b's workspace id but is routed to a — never silently applied to a's
    // sub-session, and b (never even greeted) is untouched.
    let mismatched = Message::Ops {
        workspace: b.ulid().to_u128(),
        ops: Vec::new(),
        signatures: Vec::new(),
        ranges: vec![range(9, 3, 10)],
    };
    assert_eq!(
        s.on_ops(a, &mismatched, &no_keys()),
        Err(SessionError::WorkspaceMismatch {
            called: a,
            message: b
        })
    );
    assert_eq!(
        s.state(a).unwrap(),
        SessionState::Wanting,
        "a's own state is untouched by a mismatched message"
    );
}

#[test]
fn opening_the_same_workspace_twice_never_counts_twice_against_the_cap() {
    let mut s = Session::new(dev(1), GroupId(1));
    let id = wsid(1);
    for _ in 0..3 {
        s.open_workspace(id, heads(&[])).unwrap();
    }
    assert!(s.is_open(id));
}

#[test]
fn opening_past_the_cap_is_refused() {
    let mut s = Session::new(dev(1), GroupId(1));
    for n in 0..MAX_OPEN_WORKSPACES as u128 {
        s.open_workspace(wsid(n), heads(&[])).unwrap();
    }
    let over = wsid(MAX_OPEN_WORKSPACES as u128);
    assert_eq!(
        s.open_workspace(over, heads(&[])),
        Err(SessionError::TooManyWorkspaces {
            len: MAX_OPEN_WORKSPACES + 1,
            max: MAX_OPEN_WORKSPACES
        })
    );
    assert!(!s.is_open(over));
}
