//! security-m4-review: `Hello` is the checklist's most tempting variant to have left in the
//! clear (it is sent before either side has proven anything to the other). This crate's own
//! production call path (`txtodo-daemon`'s `lan_session.rs::send_message`/`recv_message`) seals
//! *every* `Message` — `Hello` included — whole under the group key before it ever reaches a
//! `Link`, so these tests do not add new protection; they prove the protection that already
//! exists and pin it against regression. See `tasks/sync-protocol-frames/notes.md` +
//! `tasks/sync-crypto-envelope/notes.md` (this crate's own design) and `RATCHET.md`'s dated entry.

use std::collections::BTreeMap;

use crate::aead::{GroupKey, GroupKeys, SealFor, open, seal};
use crate::crypto_error::CryptoError;
use crate::frame::{Frame, PROTOCOL_VERSION};
use crate::message::{GroupId, Message, MessageError};
use crate::session::Session;
use crate::session_error::SessionError;
use txtodo_model::{DeviceId, Ulid};
use txtodo_store::WorkspaceId;

fn dev(n: u128) -> DeviceId {
    DeviceId::new(Ulid::from_u128(n))
}

fn ws() -> WorkspaceId {
    WorkspaceId::new(Ulid::from_u128(0x5EED))
}

fn a_hello(heads: BTreeMap<DeviceId, u64>) -> Message {
    Message::Hello {
        device: dev(1),
        group: GroupId(42),
        heads,
        protocol: PROTOCOL_VERSION,
        wall_ms: 1_000,
    }
}

fn seal_message(group: GroupId, key: &GroupKey, msg: &Message) -> Frame {
    let plain = msg.encode().unwrap_or_else(|e| panic!("encode: {e}"));
    let for_ = SealFor {
        group,
        epoch: 0,
        workspace: ws(),
    };
    let sealed =
        seal(plain.version, for_, key, &plain.body).unwrap_or_else(|e| panic!("seal: {e}"));
    Frame {
        version: plain.version,
        body: sealed,
    }
}

fn one_key(byte: u8) -> GroupKeys {
    let mut keys = GroupKeys::new();
    keys.insert(0, GroupKey::from_bytes([byte; 32]))
        .unwrap_or_else(|e| panic!("{e:?}"));
    keys
}

fn open_and_decode(frame: &Frame, group: GroupId, keys: &GroupKeys) -> Message {
    let plain =
        open(frame.version, group, ws(), keys, &frame.body).unwrap_or_else(|e| panic!("{e:?}"));
    Message::decode(&Frame {
        version: frame.version,
        body: plain,
    })
    .unwrap_or_else(|e| panic!("{e}"))
}

#[test]
fn a_sealed_hello_is_ciphertext_not_a_decodable_message() {
    // Confidentiality: on the wire, `Hello`'s `heads` (a privacy leak on their own — how much a
    // device has written) never appear as plaintext bytes an eavesdropper without the group key
    // can parse. The sealed body carries a 16-byte AEAD tag and a random nonce, so it is never
    // byte-identical to the plaintext encode, and decoding it directly as an unsealed `Message`
    // must fail rather than silently producing a plausible-looking one.
    let group = GroupId(42);
    let key = GroupKey::from_bytes([7; 32]);
    let hello = a_hello(BTreeMap::from([(dev(1), 5)]));
    let sealed_frame = seal_message(group, &key, &hello);
    let plain_frame = hello.encode().unwrap_or_else(|e| panic!("encode: {e}"));

    assert_ne!(
        sealed_frame.body, plain_frame.body,
        "sealed bytes must differ from the plaintext encode"
    );
    assert!(
        Message::decode(&sealed_frame).is_err(),
        "ciphertext must never happen to decode as a valid Message"
    );
}

#[test]
fn a_hello_sealed_for_one_group_cannot_be_opened_with_another_key_group_or_no_key() {
    // An eavesdropper who belongs to a *different* group (so it holds *a* group key, just not
    // this one) learns nothing from a captured Hello: `open`'s AAD binds the group id, the same
    // way `aead_tests.rs` proves for `Ops` batches — `Hello` gets no weaker treatment for being
    // sent first. Nor does holding no key at all help — named for completeness against the task
    // notes' "Hello is sent before a shared key exists" framing: in this codebase it never is
    // (`lan_session.rs::drive_session` fetches the group key before a session starts at all).
    let group = GroupId(42);
    let key = GroupKey::from_bytes([7; 32]);
    let sealed_frame = seal_message(group, &key, &a_hello(BTreeMap::new()));

    assert!(
        open(
            PROTOCOL_VERSION,
            GroupId(43),
            ws(),
            &one_key(7),
            &sealed_frame.body
        )
        .is_err()
    );
    assert!(
        open(
            PROTOCOL_VERSION,
            group,
            ws(),
            &GroupKeys::new(),
            &sealed_frame.body
        )
        .is_err()
    );
    assert!(
        open(
            PROTOCOL_VERSION,
            group,
            ws(),
            &one_key(8),
            &sealed_frame.body
        )
        .is_err()
    );
}

#[test]
fn replaying_a_captured_hello_after_the_handshake_moved_on_is_refused_and_changes_nothing() {
    // The realistic replay: an attacker records a genuine sealed Hello frame in flight and
    // resends the identical bytes later at the same session. By then the session has already
    // consumed its one legal link-level Hello and learned the peer's device id, so the replay is
    // not just useless — it is a protocol violation `Session` refuses outright, touching no state.
    // Stage 2: `Hello` is purely link-level now (no workspace, no `Want` — that moved to `Greet`),
    // so this test exercises `link_hello`/`on_link_hello` directly; the "heads/Want reveal
    // nothing new on replay" property now belongs to `Greet`, covered by
    // `session_multiplex_tests.rs`'s own greet-focused tests instead.
    let group = GroupId(42);
    let keys = one_key(7);
    let sealed_frame = seal_message(
        group,
        keys.get(0).unwrap_or_else(|| panic!("seeded")),
        &a_hello(BTreeMap::from([(dev(1), 5)])),
    );

    let mut session = Session::new(dev(2), group);
    session.link_hello(0).unwrap_or_else(|e| panic!("{e:?}"));
    session
        .on_link_hello(&open_and_decode(&sealed_frame, group, &keys), 0)
        .unwrap_or_else(|e| panic!("{e:?}"));
    let peer_after_first = session.peer();
    assert_eq!(peer_after_first, Some(dev(1)), "sanity: the Hello was accepted");

    let replay = session.on_link_hello(&open_and_decode(&sealed_frame, group, &keys), 0);
    assert_eq!(
        replay,
        Err(SessionError::LinkAlreadyGreeted),
        "a second Hello once the link handshake is done is refused, not silently reapplied"
    );
    assert_eq!(
        session.peer(),
        peer_after_first,
        "a refused replay changes nothing"
    );
}

#[test]
fn replaying_a_captured_hello_to_a_fresh_session_reveals_the_same_peer_and_skew() {
    // The other realistic replay: a *different*, freshly-started session receives the same
    // captured bytes (e.g. the attacker relays it to a new connection instead of the original
    // one). At the link level there is no per-workspace state to leak — both sessions learn
    // exactly the same, purely public facts (the sender's device id and how its clock compares),
    // and nothing about either session's own local heads is exposed (`Hello` carries none, since
    // stage 2 moved heads to the per-workspace `Greet`).
    let group = GroupId(42);
    let keys = one_key(7);
    let sealed_frame = seal_message(
        group,
        keys.get(0).unwrap_or_else(|| panic!("seeded")),
        &a_hello(BTreeMap::from([(dev(1), 5)])),
    );

    let mut victim_one = Session::new(dev(2), group);
    victim_one
        .link_hello(0)
        .unwrap_or_else(|e| panic!("{e:?}"));
    let skew_one = victim_one
        .on_link_hello(&open_and_decode(&sealed_frame, group, &keys), 0)
        .unwrap_or_else(|e| panic!("{e:?}"));

    let mut victim_two = Session::new(dev(3), group);
    victim_two
        .link_hello(0)
        .unwrap_or_else(|e| panic!("{e:?}"));
    let skew_two = victim_two
        .on_link_hello(&open_and_decode(&sealed_frame, group, &keys), 0)
        .unwrap_or_else(|e| panic!("{e:?}"));

    assert_eq!(
        skew_one, skew_two,
        "replaying the same Hello to any session yields the same public skew reading"
    );
    assert_eq!(victim_one.peer(), Some(dev(1)));
    assert_eq!(victim_two.peer(), Some(dev(1)));
}

#[test]
fn a_frame_tampered_after_sealing_a_hello_is_rejected_before_it_becomes_a_message() {
    // The same tamper-evidence proof `sealed_ops_tests.rs` runs for `Ops`, run here for `Hello`
    // specifically: the whole message lives inside one AEAD blob, so flipping a byte anywhere —
    // including inside `heads` — fails the tag rather than decoding into a plausible-looking but
    // different Hello.
    let group = GroupId(42);
    let keys = one_key(7);
    let mut sealed_frame = seal_message(
        group,
        keys.get(0).unwrap_or_else(|| panic!("seeded")),
        &a_hello(BTreeMap::from([(dev(1), 5)])),
    );
    let last = sealed_frame.body.len() - 1;
    sealed_frame.body[last] ^= 0x01;

    assert_eq!(
        open(PROTOCOL_VERSION, group, ws(), &keys, &sealed_frame.body),
        Err(CryptoError::Decrypt { epoch: 0 })
    );
}

#[test]
fn a_foreign_protocol_under_the_right_key_is_a_message_error_not_a_crypto_one() {
    // Mirrors `sealed_ops_tests.rs`'s equivalent case for the `Hello`/`Want`/`Ack` path: a
    // foreign protocol speaker who happens to hold the real group key produces bytes that open
    // fine (the AEAD tag is valid) but are not a `Message` at all — a distinct, typed failure,
    // never confused with a crypto one.
    let group = GroupId(42);
    let keys = one_key(7);
    let sealed = seal(
        PROTOCOL_VERSION,
        SealFor {
            group,
            epoch: 0,
            workspace: ws(),
        },
        keys.get(0).unwrap_or_else(|| panic!("seeded")),
        b"not a postcard message",
    )
    .unwrap_or_else(|e| panic!("seal: {e}"));
    let plain =
        open(PROTOCOL_VERSION, group, ws(), &keys, &sealed).unwrap_or_else(|e| panic!("{e:?}"));
    let frame = Frame {
        version: PROTOCOL_VERSION,
        body: plain,
    };
    assert!(matches!(
        Message::decode(&frame),
        Err(MessageError::Codec(_))
    ));
}
