//! Message: checked-in postcard goldens for every variant (a careless struct edit fails here
//! instead of silently re-encoding), frozen variant tags, and every cap enforced on both paths.
//! Regenerate goldens deliberately with `TXTODO_UPDATE_GOLDENS=1 cargo test -p txtodo-sync`.

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use crate::frame::{Frame, FrameError, PROTOCOL_VERSION};
use crate::message::{
    GroupId, MAX_HEADS, MAX_OPS_PER_BATCH, MAX_WANT_RANGES, Message, MessageError, OriginRange,
};
use crate::sign::Signature;
use txtodo_model::{DeviceId, FilePath, Hlc, Op, OpId, OpKind, Principal, TaskId, Ulid};

fn dev(n: u128) -> DeviceId {
    DeviceId::new(Ulid::from_u128(n))
}

fn range(device: u128, first: u64, last: u64) -> OriginRange {
    OriginRange {
        device: dev(device),
        first,
        last,
    }
}

/// A fixed, non-zero workspace bits fixture — these tests exercise `Message`'s shape, not the
/// typed `WorkspaceId`/`u128` conversion itself (`session_tests.rs`/`workspace_session.rs` own
/// that).
const WS: u128 = 0x5EED;

/// A deterministic, non-zero fixture signature; these tests exercise `Message`'s shape, not the
/// crypto — `sign_tests.rs`/`sealed_ops_tests.rs` own real signing.
fn sig(n: u8) -> Signature {
    Signature::from_bytes([n; 64])
}

fn op(n: u128) -> Op {
    Op {
        id: OpId::new(Ulid::from_u128(0x0100 + n)),
        hlc: Hlc {
            wall_ms: 1_700_000_000_000 + n as u64,
            counter: 0,
            device: dev(1),
        },
        principal: Principal::User { device: dev(1) },
        file: FilePath::new("todo.txt").unwrap(),
        kind: OpKind::Insert {
            task: TaskId::new(Ulid::from_u128(0x0200 + n)),
            after: None,
            line: format!("task {n} id:{}", Ulid::from_u128(0x0200 + n)),
        },
    }
}

fn hello() -> Message {
    let mut heads = BTreeMap::new();
    heads.insert(dev(1), 42);
    heads.insert(dev(2), 7);
    Message::Hello {
        device: dev(1),
        group: GroupId(0xABCD),
        heads,
        protocol: PROTOCOL_VERSION,
        wall_ms: 1_700_000_000_000,
    }
}

/// Encodes `msg`, compares against `goldens/<name>.postcard`, and round-trips it.
fn golden(name: &str, msg: &Message) {
    let frame = msg.encode().unwrap();
    let path: PathBuf = [
        env!("CARGO_MANIFEST_DIR"),
        "goldens",
        &format!("{name}.postcard"),
    ]
    .iter()
    .collect();
    // Test-only seam: cfg(test) code cannot ship, and the variable must be set on purpose.
    if std::env::var_os("TXTODO_UPDATE_GOLDENS").is_some() {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, &frame.body).unwrap();
    }
    let want = fs::read(&path)
        .unwrap_or_else(|e| panic!("missing golden {}: {e}; see module doc", path.display()));
    assert_eq!(
        frame.body, want,
        "{name}: wire bytes changed — a wire break, not a refactor"
    );
    assert_eq!(Message::decode(&frame).unwrap(), *msg, "{name} round-trips");
}

#[test]
fn every_message_matches_its_checked_in_golden() {
    golden("hello", &hello());
    golden(
        "want",
        &Message::Want {
            workspace: WS,
            ranges: vec![range(2, 8, 12), range(3, 1, 1)],
        },
    );
    golden(
        "ops",
        &Message::Ops {
            workspace: WS,
            ops: vec![op(1), op(2)],
            signatures: vec![sig(1), sig(2)],
            ranges: vec![range(1, 43, 44)],
        },
    );
    golden(
        "ack",
        &Message::Ack {
            workspace: WS,
            committed: vec![range(1, 43, 44)],
        },
    );
    golden(
        "want_empty",
        &Message::Want {
            workspace: WS,
            ranges: Vec::new(),
        },
    );
}

#[test]
fn variant_tags_are_frozen_in_declaration_order() {
    let tags: Vec<u8> = [
        hello(),
        Message::Want {
            workspace: WS,
            ranges: Vec::new(),
        },
        Message::Ops {
            workspace: WS,
            ops: Vec::new(),
            signatures: Vec::new(),
            ranges: Vec::new(),
        },
        Message::Ack {
            workspace: WS,
            committed: Vec::new(),
        },
    ]
    .iter()
    .map(|m| m.encode().unwrap().body[0])
    .collect();
    // postcard writes the variant index first; appending a variant keeps these, inserting breaks them.
    assert_eq!(tags, vec![0, 1, 2, 3]);
    assert_eq!(
        Message::Ack {
            workspace: 0,
            committed: Vec::new()
        }
        .encode()
        .unwrap()
        .body,
        vec![3, 0, 0],
        "tag, then workspace 0's varint, then an empty vec length"
    );
}

#[test]
fn caps_are_checked_before_encode() {
    let too_many_heads: BTreeMap<DeviceId, u64> =
        (0..=MAX_HEADS as u128).map(|n| (dev(n), 1)).collect();
    let hello = Message::Hello {
        device: dev(1),
        group: GroupId(1),
        heads: too_many_heads,
        protocol: PROTOCOL_VERSION,
        wall_ms: 0,
    };
    assert_eq!(
        hello.encode().unwrap_err(),
        MessageError::TooMany {
            what: "heads",
            len: MAX_HEADS + 1,
            max: MAX_HEADS
        }
    );
    let too_many_ops: Vec<Op> = (0..=MAX_OPS_PER_BATCH as u128).map(op).collect();
    let ops = Message::Ops {
        workspace: WS,
        signatures: too_many_ops.iter().map(|_| sig(0)).collect(),
        ops: too_many_ops,
        ranges: Vec::new(),
    };
    assert!(matches!(
        ops.encode(),
        Err(MessageError::TooMany { what: "ops", .. })
    ));
    let mismatched = Message::Ops {
        workspace: WS,
        ops: vec![op(1)],
        signatures: Vec::new(),
        ranges: Vec::new(),
    };
    assert_eq!(
        mismatched.encode().unwrap_err(),
        MessageError::SignatureCount {
            ops: 1,
            signatures: 0
        }
    );
    let backwards = Message::Ack {
        workspace: WS,
        committed: vec![range(1, 5, 4)],
    };
    assert_eq!(
        backwards.encode().unwrap_err(),
        MessageError::BackwardsRange(range(1, 5, 4))
    );
}

#[test]
fn caps_are_checked_after_decode_too() {
    // Bypass encode()'s check the way a hostile peer would: raw postcard into a frame.
    let oversized = Message::Want {
        workspace: WS,
        ranges: (0..=MAX_WANT_RANGES as u64)
            .map(|n| range(9, n, n))
            .collect(),
    };
    let frame = Frame::new(postcard::to_allocvec(&oversized).unwrap()).unwrap();
    assert_eq!(
        Message::decode(&frame).unwrap_err(),
        MessageError::TooMany {
            what: "want ranges",
            len: MAX_WANT_RANGES + 1,
            max: MAX_WANT_RANGES
        }
    );
    let backwards = Message::Want {
        workspace: WS,
        ranges: vec![range(1, 2, 1)],
    };
    let frame = Frame::new(postcard::to_allocvec(&backwards).unwrap()).unwrap();
    assert_eq!(
        Message::decode(&frame).unwrap_err(),
        MessageError::BackwardsRange(range(1, 2, 1))
    );
}

#[test]
fn trailing_bytes_garbage_and_other_versions_are_refused() {
    let mut frame = hello().encode().unwrap();
    frame.body.push(0);
    assert_eq!(
        Message::decode(&frame).unwrap_err(),
        MessageError::TrailingBytes(1)
    );
    let garbage = Frame::new(vec![0xFF, 0xFF, 0xFF]).unwrap();
    assert!(matches!(
        Message::decode(&garbage).unwrap_err(),
        MessageError::Codec(_)
    ));
    let mut next = hello().encode().unwrap();
    next.version = PROTOCOL_VERSION + 1;
    assert_eq!(
        Message::decode(&next).unwrap_err(),
        MessageError::Frame(FrameError::UnknownVersion {
            got: PROTOCOL_VERSION + 1,
            supported: PROTOCOL_VERSION
        })
    );
}
