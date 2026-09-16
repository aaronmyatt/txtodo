//! `sync-reject-tests`: proves the actual op send/receive path — not just the isolated
//! primitives `sign_tests.rs`/`aead_tests.rs` already cover — rejects (a) a peer without the
//! correct group key and (b) a tampered op. Two in-process peers, real key material, no sockets
//! and no second daemon process: the two-daemon integration variant is a separate, still-open
//! task (see `tasks/sync-reject-tests/notes.md`).

use std::collections::BTreeMap;

use txtodo_model::{DeviceId, FilePath, Hlc, Op, OpId, OpKind, Principal, TaskId, Ulid};
use txtodo_store::WorkspaceId;

use crate::aead::{GroupKey, GroupKeys, SealFor};
use crate::crypto_error::CryptoError;
use crate::frame::PROTOCOL_VERSION;
use crate::message::{GroupId, Heads, Message, MessageError, OriginRange};
use crate::sealed_ops::{SealContext, SealedOpsError, open_ops, seal_ops};
use crate::session::{Session, SessionState};
use crate::session_error::SessionError;
use crate::sign::{DevicePublicKey, DeviceSigningKey};

fn dev(n: u128) -> DeviceId {
    DeviceId::new(Ulid::from_u128(n))
}

fn signing_key(seed: u8) -> DeviceSigningKey {
    DeviceSigningKey::from_bytes([seed; 32])
}

fn group_key(byte: u8) -> GroupKey {
    GroupKey::from_bytes([byte; 32])
}

fn ws() -> WorkspaceId {
    WorkspaceId::new(Ulid::from_u128(0x5EED))
}

/// Stage 2: a workspace's `Greet` requires the link-level `Hello` handshake done first
/// (`Session::on_hello`'s own `LinkNotReady` guard) — drives both steps so `on_ops`-focused tests
/// can reach `Wanting` without re-deriving this sequence three times.
fn greeted_and_wanting(
    local: DeviceId,
    group: GroupId,
    sender: DeviceId,
    sender_heads: Heads,
) -> Session {
    let mut session = Session::new(local, group);
    session.open_workspace(ws(), BTreeMap::new()).unwrap();
    session.link_hello(0).unwrap();
    session
        .on_link_hello(
            &Message::Hello {
                device: sender,
                group,
                heads: BTreeMap::new(),
                protocol: PROTOCOL_VERSION,
                wall_ms: 0,
            },
            0,
        )
        .unwrap();
    session.hello(ws()).unwrap();
    session
        .on_hello(
            ws(),
            &Message::Greet {
                workspace: ws().ulid().to_u128(),
                heads: sender_heads,
            },
        )
        .unwrap();
    session
}

fn op(device: DeviceId, line: &str) -> Op {
    Op {
        id: OpId::new(Ulid::from_u128(0x0100)),
        hlc: Hlc {
            wall_ms: 1_700_000_000_000,
            counter: 0,
            device,
        },
        principal: Principal::User { device },
        file: FilePath::new("todo.txt").unwrap(),
        kind: OpKind::Insert {
            task: TaskId::new(Ulid::from_u128(0x0200)),
            after: None,
            line: line.to_owned(),
        },
    }
}

/// A victim's world: the real group key at epoch 0, and the sender's real public key.
struct Victim {
    group_keys: GroupKeys,
    device_keys: BTreeMap<DeviceId, DevicePublicKey>,
}

fn victim(sender: DeviceId, sender_key: &DeviceSigningKey) -> Victim {
    let mut group_keys = GroupKeys::new();
    group_keys.insert(0, group_key(0x11)).unwrap();
    let mut device_keys = BTreeMap::new();
    device_keys.insert(sender, sender_key.public_key());
    Victim {
        group_keys,
        device_keys,
    }
}

#[test]
fn a_genuine_sealed_batch_opens_and_flows_straight_into_session_on_ops() {
    // Positive control: the wiring this task adds must not break the happy path.
    let sender = dev(1);
    let sender_key = signing_key(7);
    let v = victim(sender, &sender_key);
    let ctx = SealContext {
        group: GroupId(42),
        epoch: 0,
        workspace: ws(),
        key: v.group_keys.get(0).unwrap(),
    };
    let range = OriginRange {
        device: sender,
        first: 1,
        last: 1,
    };
    let frame = seal_ops(vec![op(sender, "buy milk")], vec![range], &sender_key, &ctx).unwrap();

    let msg = open_ops(&frame, GroupId(42), ws(), &v.group_keys, &v.device_keys).unwrap();

    let mut session =
        greeted_and_wanting(dev(2), GroupId(42), sender, BTreeMap::from([(sender, 1)]));
    let ops = session.on_ops(ws(), &msg, &v.device_keys).unwrap();
    assert_eq!(ops.len(), 1, "the genuine op reaches the session");
    assert_eq!(session.state(ws()).unwrap(), SessionState::Importing);
}

#[test]
fn a_peer_sealing_with_the_wrong_group_key_is_rejected() {
    // The attacker knows the group id (it is public in the mDNS TXT record by design) but not
    // the actual group key — it seals with something else entirely.
    let sender = dev(1);
    let sender_key = signing_key(7);
    let v = victim(sender, &sender_key);
    let wrong_key = group_key(0xEE);
    let ctx = SealContext {
        group: GroupId(42),
        epoch: 0,
        workspace: ws(),
        key: &wrong_key,
    };
    let range = OriginRange {
        device: sender,
        first: 1,
        last: 1,
    };
    let frame = seal_ops(vec![op(sender, "buy milk")], vec![range], &sender_key, &ctx).unwrap();

    assert_eq!(
        open_ops(&frame, GroupId(42), ws(), &v.group_keys, &v.device_keys),
        Err(SealedOpsError::Crypto(CryptoError::Decrypt { epoch: 0 })),
    );
}

#[test]
fn a_peer_with_no_group_key_at_all_is_rejected() {
    // A device that was never paired into the group holds no key for any epoch — the failure is
    // "no key retained", never a decrypt attempt against the wrong one.
    let sender = dev(1);
    let sender_key = signing_key(7);
    let v = victim(sender, &sender_key);
    let unrelated_key = group_key(0xFF);
    let ctx = SealContext {
        group: GroupId(42),
        epoch: 0,
        workspace: ws(),
        key: &unrelated_key,
    };
    let frame = seal_ops(vec![op(sender, "buy milk")], vec![], &sender_key, &ctx).unwrap();

    let unkeyed_victim = GroupKeys::new();
    assert_eq!(
        open_ops(&frame, GroupId(42), ws(), &unkeyed_victim, &v.device_keys),
        Err(SealedOpsError::Crypto(CryptoError::UnknownEpoch {
            epoch: 0,
            held: 0
        })),
    );
}

#[test]
fn a_frame_tampered_after_sealing_is_rejected_before_any_op_is_read() {
    // The wire-level tamper: capture the sealed frame a genuine sender produced, flip one byte,
    // forward it. Every byte lives inside one AEAD blob, so any flip fails the tag — the batch
    // never reaches signature checking, let alone `Session`.
    let sender = dev(1);
    let sender_key = signing_key(7);
    let v = victim(sender, &sender_key);
    let ctx = SealContext {
        group: GroupId(42),
        epoch: 0,
        workspace: ws(),
        key: v.group_keys.get(0).unwrap(),
    };
    let range = OriginRange {
        device: sender,
        first: 1,
        last: 1,
    };
    let mut frame = seal_ops(vec![op(sender, "buy milk")], vec![range], &sender_key, &ctx).unwrap();
    let last = frame.body.len() - 1;
    frame.body[last] ^= 0x01;

    assert_eq!(
        open_ops(&frame, GroupId(42), ws(), &v.group_keys, &v.device_keys),
        Err(SealedOpsError::Crypto(CryptoError::Decrypt { epoch: 0 })),
    );
}

#[test]
fn session_on_ops_rejects_a_signed_batch_tampered_after_signing() {
    // Exercises `Session::on_ops`'s own wiring directly (no seal/open involved): a message that
    // was never sealed, but whose op content no longer matches what was signed.
    let sender = dev(1);
    let sender_key = signing_key(7);
    let signature = crate::sign::sign(&op(sender, "buy milk"), &sender_key).unwrap();
    let tampered = Message::Ops {
        workspace: ws().ulid().to_u128(),
        ops: vec![op(sender, "buy SILK")],
        signatures: vec![signature],
        ranges: vec![OriginRange {
            device: sender,
            first: 1,
            last: 1,
        }],
    };
    let mut device_keys = BTreeMap::new();
    device_keys.insert(sender, sender_key.public_key());

    let mut session =
        greeted_and_wanting(dev(2), GroupId(42), sender, BTreeMap::from([(sender, 1)]));
    assert_eq!(
        session.on_ops(ws(), &tampered, &device_keys),
        Err(SessionError::Crypto(CryptoError::SignatureInvalid {
            device: sender
        })),
    );
    assert_eq!(
        session.state(ws()).unwrap(),
        SessionState::Wanting,
        "a rejected batch never enters Importing"
    );
}

#[test]
fn session_on_ops_rejects_a_batch_from_a_device_with_no_known_key() {
    let sender = dev(1);
    let unsigned_by = signing_key(9);
    let signature = crate::sign::sign(&op(sender, "buy milk"), &unsigned_by).unwrap();
    let msg = Message::Ops {
        workspace: ws().ulid().to_u128(),
        ops: vec![op(sender, "buy milk")],
        signatures: vec![signature],
        ranges: vec![OriginRange {
            device: sender,
            first: 1,
            last: 1,
        }],
    };

    let mut session =
        greeted_and_wanting(dev(2), GroupId(42), sender, BTreeMap::from([(sender, 1)]));
    // No entry at all for `sender`: a missing key is refused, never skipped.
    assert_eq!(
        session.on_ops(ws(), &msg, &BTreeMap::new()),
        Err(SessionError::Crypto(CryptoError::UnknownDevice {
            device: sender
        })),
    );
}

#[test]
fn open_ops_surfaces_a_message_error_when_the_sealed_plaintext_is_not_a_valid_message() {
    // A batch that opens (the AEAD tag is fine) but whose plaintext is garbage — e.g. a peer
    // speaking a foreign protocol under the same group key — is a `Message` error, not a crypto
    // one; `SealedOpsError` keeps the two distinguishable.
    let group_keys = {
        let mut g = GroupKeys::new();
        g.insert(0, group_key(0x11)).unwrap();
        g
    };
    let sealed = crate::aead::seal(
        PROTOCOL_VERSION,
        SealFor {
            group: GroupId(42),
            epoch: 0,
            workspace: ws(),
        },
        group_keys.get(0).unwrap(),
        b"not a postcard message",
    )
    .unwrap();
    let frame = crate::frame::Frame {
        version: PROTOCOL_VERSION,
        body: sealed,
    };
    assert!(matches!(
        open_ops(&frame, GroupId(42), ws(), &group_keys, &BTreeMap::new()),
        Err(SealedOpsError::Message(MessageError::Codec(_)))
    ));
}
