//! `load_or_mint_relay_identity` (task `daemon-workspace-identity-agreement` stage 1): mints once,
//! then keeps returning the same seed — the property a stable relay node id across daemon restarts
//! actually depends on (`txtodo-sync`'s `holepunch_tests.rs` proves the other half: that binding
//! with the same seed twice yields the same `node_id_bytes()`).

use txtodo_sync::{FileKeyStore, MemoryKeyStore, Secret};

use crate::keystore_setup::load_or_mint_relay_identity;

#[test]
fn mints_once_and_then_stays_stable() {
    let store = MemoryKeyStore::default();
    let first = load_or_mint_relay_identity(&store).unwrap();
    let second = load_or_mint_relay_identity(&store).unwrap();
    assert_eq!(first, second);
}

#[test]
fn two_separate_keystores_mint_different_seeds() {
    let a = load_or_mint_relay_identity(&MemoryKeyStore::default()).unwrap();
    let b = load_or_mint_relay_identity(&MemoryKeyStore::default()).unwrap();
    assert_ne!(a, b);
}

/// The actual "survives a daemon restart" proof: a real on-disk `FileKeyStore`, reopened (not the
/// same in-memory instance) the way a second `DeviceIdentity::open` on the same `state_dir` would —
/// combined with `txtodo-sync`'s `bind_with_secret_key_is_stable_across_binds`, this is what makes
/// a restarted daemon's relay node id actually stay put instead of only "the same object twice".
#[test]
fn survives_a_real_keystore_file_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("keystore");
    let passphrase = Secret::new(b"test-passphrase".to_vec());

    let created = FileKeyStore::create(&path, &passphrase).unwrap();
    let minted = load_or_mint_relay_identity(&created).unwrap();
    drop(created);

    let reopened = FileKeyStore::open(&path, &passphrase).unwrap();
    let loaded = load_or_mint_relay_identity(&reopened).unwrap();
    assert_eq!(minted, loaded);
}
