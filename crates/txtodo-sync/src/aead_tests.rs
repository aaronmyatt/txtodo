//! AEAD tests: round trip, fresh nonces, tamper detection, and the context failures (wrong key,
//! wrong group, wrong epoch, wrong workspace) each giving their own typed error.

use crate::aead::{
    GroupKey, GroupKeys, MAX_RETAINED_KEY_EPOCHS, SEALED_HEADER_BYTES, SealFor, open,
    peek_workspace, seal,
};
use crate::crypto_error::CryptoError;
use crate::frame::PROTOCOL_VERSION;
use crate::message::GroupId;
use txtodo_model::Ulid;
use txtodo_store::WorkspaceId;

fn key(byte: u8) -> GroupKey {
    GroupKey::from_bytes([byte; 32])
}

fn one_epoch(epoch: u32, key_byte: u8) -> GroupKeys {
    let mut keys = GroupKeys::new();
    keys.insert(epoch, key(key_byte)).unwrap();
    keys
}

fn workspace(n: u128) -> WorkspaceId {
    WorkspaceId::new(Ulid::from_u128(n))
}

fn for_(group: GroupId, epoch: u32, ws: WorkspaceId) -> SealFor {
    SealFor {
        group,
        epoch,
        workspace: ws,
    }
}

#[test]
fn seal_and_open_round_trip_with_the_header_in_the_clear() {
    let group = GroupId(0xABCD);
    let ws = workspace(1);
    let keys = one_epoch(3, 7);
    let sealed = seal(
        PROTOCOL_VERSION,
        for_(group, 3, ws),
        keys.get(3).unwrap(),
        b"ops batch",
    )
    .unwrap();
    assert_eq!(u16::from_le_bytes([sealed[0], sealed[1]]), PROTOCOL_VERSION);
    assert_eq!(sealed.len(), SEALED_HEADER_BYTES + b"ops batch".len() + 16);
    assert_eq!(
        open(PROTOCOL_VERSION, group, ws, &keys, &sealed).unwrap(),
        b"ops batch"
    );
}

#[test]
fn nonces_are_fresh_from_the_os_and_not_a_seeded_prng() {
    // `seal` takes no RNG argument, so the deterministic simulator cannot inject one; the observable
    // proof is that 100 seals of identical bytes never repeat a header.
    let group = GroupId(1);
    let ws = workspace(1);
    let keys = one_epoch(0, 7);
    let mut headers = std::collections::BTreeSet::new();
    for _ in 0..100 {
        let sealed = seal(
            PROTOCOL_VERSION,
            for_(group, 0, ws),
            keys.get(0).unwrap(),
            b"same",
        )
        .unwrap();
        assert!(headers.insert(sealed[..SEALED_HEADER_BYTES].to_vec()));
    }
    assert_eq!(headers.len(), 100);
}

#[test]
fn flipping_one_ciphertext_byte_fails_the_tag() {
    let group = GroupId(1);
    let ws = workspace(1);
    let keys = one_epoch(0, 7);
    let mut sealed = seal(
        PROTOCOL_VERSION,
        for_(group, 0, ws),
        keys.get(0).unwrap(),
        b"ops",
    )
    .unwrap();
    let last = sealed.len() - 1;
    sealed[last] ^= 0x01;
    assert_eq!(
        open(PROTOCOL_VERSION, group, ws, &keys, &sealed),
        Err(CryptoError::Decrypt { epoch: 0 })
    );
}

#[test]
fn a_wrong_group_key_fails_the_tag_but_an_unknown_epoch_is_a_distinct_error() {
    let group = GroupId(1);
    let ws = workspace(1);
    // Same epoch number, different key bytes: the header parses, the tag fails.
    assert_eq!(
        open(
            PROTOCOL_VERSION,
            group,
            ws,
            &one_epoch(0, 8),
            &seal(PROTOCOL_VERSION, for_(group, 0, ws), &key(7), b"ops").unwrap()
        ),
        Err(CryptoError::Decrypt { epoch: 0 })
    );
    // No key for epoch 5 at all: named, not guessed at.
    assert_eq!(
        open(
            PROTOCOL_VERSION,
            group,
            ws,
            &one_epoch(0, 7),
            &seal(PROTOCOL_VERSION, for_(group, 5, ws), &key(7), b"ops").unwrap()
        ),
        Err(CryptoError::UnknownEpoch { epoch: 5, held: 1 })
    );
}

#[test]
fn a_foreign_group_fails_the_aad_before_the_ciphertext_is_touched() {
    let ws = workspace(1);
    let sealed = seal(PROTOCOL_VERSION, for_(GroupId(1), 0, ws), &key(7), b"ops").unwrap();
    assert_eq!(
        open(PROTOCOL_VERSION, GroupId(2), ws, &one_epoch(0, 7), &sealed),
        Err(CryptoError::WrongGroup {
            got: GroupId(1),
            expected: GroupId(2)
        })
    );
}

#[test]
fn a_mislabelled_workspace_fails_the_aad_even_with_the_same_group_and_key() {
    // ADR 0021: every workspace on a device now shares one group key, so the workspace tag is the
    // only thing telling two workspaces' batches apart — this proves it is authenticated, not a
    // bare prefix a bug (or an active relay) could swap without detection.
    let group = GroupId(1);
    let sealed = seal(
        PROTOCOL_VERSION,
        for_(group, 0, workspace(1)),
        &key(7),
        b"ops",
    )
    .unwrap();
    assert_eq!(
        open(
            PROTOCOL_VERSION,
            group,
            workspace(2),
            &one_epoch(0, 7),
            &sealed
        ),
        Err(CryptoError::WrongWorkspace {
            got: workspace(1),
            expected: workspace(2)
        })
    );
}

#[test]
fn a_downgraded_version_fails_the_aad_distinctly() {
    let ws = workspace(1);
    let sealed = seal(PROTOCOL_VERSION, for_(GroupId(1), 0, ws), &key(7), b"ops").unwrap();
    assert_eq!(
        open(
            PROTOCOL_VERSION + 1,
            GroupId(1),
            ws,
            &one_epoch(0, 7),
            &sealed
        ),
        Err(CryptoError::WrongVersion {
            got: PROTOCOL_VERSION,
            expected: PROTOCOL_VERSION + 1
        })
    );
}

#[test]
fn a_short_blob_is_refused_before_any_indexing() {
    let keys = one_epoch(0, 7);
    for len in 0..(SEALED_HEADER_BYTES + 16) {
        assert_eq!(
            open(
                PROTOCOL_VERSION,
                GroupId(1),
                workspace(1),
                &keys,
                &vec![0u8; len]
            ),
            Err(CryptoError::Truncated {
                needed: SEALED_HEADER_BYTES + 16,
                got: len
            }),
            "len {len}"
        );
    }
}

#[test]
fn retaining_more_than_the_cap_is_refused_and_replacing_is_allowed() {
    let mut keys = GroupKeys::new();
    for epoch in 0..MAX_RETAINED_KEY_EPOCHS as u32 {
        keys.insert(epoch, key(epoch as u8)).unwrap();
    }
    assert_eq!(keys.len(), MAX_RETAINED_KEY_EPOCHS);
    assert_eq!(
        keys.insert(999, key(1)),
        Err(CryptoError::TooManyEpochs {
            len: MAX_RETAINED_KEY_EPOCHS + 1,
            max: MAX_RETAINED_KEY_EPOCHS
        })
    );
    // Overwriting an epoch already held never grows the set.
    assert!(keys.insert(0, key(0xFF)).is_ok());
    assert_eq!(keys.len(), MAX_RETAINED_KEY_EPOCHS);
    assert!(keys.get(0).is_some());
    assert!(keys.get(999).is_none());
    assert!(!keys.is_empty());
}

#[test]
fn peek_workspace_reads_the_clear_header_without_a_key() {
    let group = GroupId(1);
    let ws = workspace(42);
    let sealed = seal(PROTOCOL_VERSION, for_(group, 0, ws), &key(7), b"ops").unwrap();
    assert_eq!(peek_workspace(&sealed), Some(ws));
}

#[test]
fn peek_workspace_is_none_on_anything_shorter_than_the_header_not_a_panic() {
    for len in 0..38 {
        assert_eq!(peek_workspace(&vec![0u8; len]), None, "len {len}");
    }
}

#[test]
fn peek_workspace_is_routing_not_authentication() {
    // A peeked id is exactly the bytes in the clear header — it is not proven correct the way
    // `open`'s own `WrongWorkspace` check proves one. Flipping the clear header's workspace bytes
    // (never touching the ciphertext or tag) changes what a peek reports; `open` against the real
    // expected id still (correctly) refuses it, proving the peek alone was never the security
    // boundary — see this function's own name for why that's the point being tested.
    let group = GroupId(1);
    let real_ws = workspace(1);
    let claimed_ws = workspace(2);
    let mut sealed = seal(PROTOCOL_VERSION, for_(group, 0, real_ws), &key(7), b"ops").unwrap();
    sealed[22..38].copy_from_slice(&claimed_ws.ulid().to_u128().to_le_bytes());
    assert_eq!(
        peek_workspace(&sealed),
        Some(claimed_ws),
        "the peek trusts whatever bytes are there"
    );
    assert_eq!(
        open(
            PROTOCOL_VERSION,
            group,
            claimed_ws,
            &one_epoch(0, 7),
            &sealed
        ),
        Err(CryptoError::Decrypt { epoch: 0 }),
        "but the real open() still fails the tag once the AAD no longer matches the ciphertext"
    );
}

#[test]
fn a_group_key_debug_never_prints_key_material() {
    let k = key(0xAB);
    assert_eq!(format!("{k:?}"), "GroupKey(<redacted>)");
    assert!(!format!("{k:?}").contains("ab"));
}
