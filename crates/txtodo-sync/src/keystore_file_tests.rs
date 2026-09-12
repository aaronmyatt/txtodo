//! Encrypted-file backend: permission refusal, Argon2 header round-trip, wrong-passphrase, and the
//! "no file appears" assertion the resolver test also leans on.

// `check_permissions` (keystore_file.rs) is a no-op on non-Unix targets, so the mode-bit
// assertions below only make sense on Unix; `std::os::unix` itself does not exist elsewhere.
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

use crate::keystore::{KeyId, KeyStore, Secret};
use crate::keystore_error::KeyStoreError;
use crate::keystore_file::FileKeyStore;

fn tmp_path(name: &str) -> std::path::PathBuf {
    let mut p = std::env::temp_dir();
    p.push(format!(
        "txtodo-keystore-test-{name}-{}-{:?}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
    ));
    p
}

#[test]
fn create_then_open_round_trips_every_key_id() {
    let path = tmp_path("roundtrip");
    let pass = Secret::new(b"correct horse battery staple".to_vec());
    let store = FileKeyStore::create(&path, &pass).unwrap();
    for (i, id) in [KeyId::DeviceSigning, KeyId::DeviceStatic, KeyId::Group(7)]
        .into_iter()
        .enumerate()
    {
        store.put(id, &Secret::new(vec![i as u8; 4])).unwrap();
    }
    drop(store);

    let reopened = FileKeyStore::open(&path, &pass).unwrap();
    assert_eq!(
        reopened.get(KeyId::DeviceSigning).unwrap(),
        Some(Secret::new(vec![0, 0, 0, 0]))
    );
    assert_eq!(
        reopened.get(KeyId::Group(7)).unwrap(),
        Some(Secret::new(vec![2, 2, 2, 2]))
    );
    let _ = std::fs::remove_file(&path);
}

#[test]
fn create_refuses_to_overwrite_an_existing_file() {
    let path = tmp_path("noclobber");
    let pass = Secret::new(b"first passphrase".to_vec());
    FileKeyStore::create(&path, &pass).unwrap();
    let original = std::fs::read(&path).unwrap();

    let other = Secret::new(b"second passphrase".to_vec());
    assert!(FileKeyStore::create(&path, &other).is_err());
    // The original file, and whatever it holds, must be untouched by the refused attempt.
    assert_eq!(std::fs::read(&path).unwrap(), original);
    assert!(FileKeyStore::open(&path, &pass).is_ok());
    let _ = std::fs::remove_file(&path);
}

#[test]
fn wrong_passphrase_is_refused_not_corrupted() {
    let path = tmp_path("wrongpass");
    let right = Secret::new(b"right passphrase".to_vec());
    let wrong = Secret::new(b"wrong passphrase".to_vec());
    FileKeyStore::create(&path, &right).unwrap();

    let err = FileKeyStore::open(&path, &wrong).unwrap_err();
    assert_eq!(err, KeyStoreError::WrongPassphrase { path: path.clone() });
    let _ = std::fs::remove_file(&path);
}

#[cfg(unix)]
#[test]
fn group_or_world_readable_file_is_refused_not_repaired() {
    let path = tmp_path("perms");
    let pass = Secret::new(b"passphrase".to_vec());
    FileKeyStore::create(&path, &pass).unwrap();

    let mut perms = std::fs::metadata(&path).unwrap().permissions();
    perms.set_mode(0o644);
    std::fs::set_permissions(&path, perms).unwrap();

    let err = FileKeyStore::open(&path, &pass).unwrap_err();
    match err {
        KeyStoreError::PermissionsTooOpen { path: p, mode } => {
            assert_eq!(p, path);
            assert_eq!(mode & 0o077, 0o044);
        }
        other => panic!("expected PermissionsTooOpen, got {other:?}"),
    }
    // Refused, not silently chmod'd back — the file's mode is unchanged by the failed open.
    let mode_after = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode_after, 0o644);
    let _ = std::fs::remove_file(&path);
}

#[cfg(unix)]
#[test]
fn created_file_has_owner_only_permissions() {
    let path = tmp_path("createmode");
    let pass = Secret::new(b"passphrase".to_vec());
    FileKeyStore::create(&path, &pass).unwrap();
    let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600);
    let _ = std::fs::remove_file(&path);
}

#[cfg(unix)]
#[test]
fn low_argon2_parameters_still_open_after_constants_are_raised() {
    // Simulates "raised the constants later": derive+seal by hand with deliberately tiny
    // parameters, confirming `open` reads params from the header rather than the current consts.
    use argon2::{Algorithm, Argon2, Params, Version};
    use chacha20poly1305::aead::{Aead, KeyInit, Payload};
    use chacha20poly1305::{XChaCha20Poly1305, XNonce};

    let path = tmp_path("lowparams");
    let pass = Secret::new(b"passphrase".to_vec());
    let salt = [7u8; 16];
    let (memory_kib, iterations, parallelism) = (8, 1, 1); // far below today's constants
    let params = Params::new(memory_kib, iterations, parallelism, Some(32)).unwrap();
    let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
    let mut key = [0u8; 32];
    argon2
        .hash_password_into(pass.expose(), &salt, &mut key)
        .unwrap();

    let mut header = [0u8; 34];
    header[0..4].copy_from_slice(b"TXKS");
    header[4..6].copy_from_slice(&1u16.to_le_bytes());
    header[6..10].copy_from_slice(&memory_kib.to_le_bytes());
    header[10..14].copy_from_slice(&iterations.to_le_bytes());
    header[14..18].copy_from_slice(&parallelism.to_le_bytes());
    header[18..34].copy_from_slice(&salt);

    let nonce = [1u8; 24];
    let plaintext =
        postcard::to_allocvec(&std::collections::BTreeMap::<KeyId, Vec<u8>>::new()).unwrap();
    let ciphertext = XChaCha20Poly1305::new((&key).into())
        .encrypt(
            XNonce::from_slice(&nonce),
            Payload {
                msg: &plaintext,
                aad: &header,
            },
        )
        .unwrap();
    let mut out = header.to_vec();
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&ciphertext);
    std::fs::write(&path, &out).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();

    // Today's constants (ARGON2_MEMORY_KIB etc.) are much higher; open must still succeed.
    let store = FileKeyStore::open(&path, &pass).unwrap();
    assert_eq!(store.get(KeyId::DeviceSigning).unwrap(), None);
    let _ = std::fs::remove_file(&path);
}
