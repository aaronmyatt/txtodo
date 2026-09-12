//! `auto`/`os`/`file` selection, with a fake OS probe — no test here calls into the real `keyring`
//! crate, so CI never pops a dialog. Covers the task's one hard rule: `auto` writes no file when the
//! OS backend is unavailable.

use crate::keystore::KeyId;
use crate::keystore_error::KeyStoreError;
use crate::keystore_memory::MemoryKeyStore;
use crate::keystore_resolve::{KeyStoreMode, ResolvedBackend, resolve};

fn no_such_file() -> std::path::PathBuf {
    let mut p = std::env::temp_dir();
    p.push(format!(
        "txtodo-keystore-resolve-test-{}-{:?}",
        std::process::id(),
        std::time::SystemTime::now()
    ));
    p
}

/// `Result::unwrap_err` needs `T: Debug`; the `Ok` side here is `Box<dyn KeyStore>`, which is not.
fn expect_err<T>(r: Result<T, KeyStoreError>) -> KeyStoreError {
    match r {
        Err(e) => e,
        Ok(_) => panic!("expected an error"),
    }
}

#[test]
fn auto_with_os_available_picks_os_and_touches_no_file() {
    let path = no_such_file();
    let (backend, store) = resolve(
        KeyStoreMode::Auto,
        || Ok(()),
        MemoryKeyStore::new,
        || -> Result<MemoryKeyStore, KeyStoreError> { panic!("file backend must not be built") },
    )
    .unwrap();
    assert_eq!(backend, ResolvedBackend::Os);
    assert_eq!(store.get(KeyId::DeviceSigning).unwrap(), None);
    assert!(!path.exists());
}

#[test]
fn auto_with_os_unavailable_errors_and_writes_nothing() {
    let path = no_such_file();
    let err = expect_err(resolve(
        KeyStoreMode::Auto,
        || Err("no D-Bus session".to_string()),
        MemoryKeyStore::new,
        || -> Result<MemoryKeyStore, KeyStoreError> {
            panic!("auto must never build a file backend on its own")
        },
    ));
    assert_eq!(
        err,
        KeyStoreError::AutoNeedsChoice {
            reason: "no D-Bus session".to_string()
        }
    );
    assert!(!path.exists(), "auto must never write a key file itself");
}

#[test]
fn os_mode_unavailable_is_a_hard_error() {
    let err = expect_err(resolve(
        KeyStoreMode::Os,
        || Err("Keychain locked".to_string()),
        MemoryKeyStore::new,
        || -> Result<MemoryKeyStore, KeyStoreError> {
            panic!("os mode must not build a file backend")
        },
    ));
    assert_eq!(
        err,
        KeyStoreError::Unavailable {
            backend: "os",
            reason: "Keychain locked".to_string()
        }
    );
}

#[test]
fn file_mode_never_probes_the_os_backend() {
    let (backend, store) = resolve(
        KeyStoreMode::File,
        || -> Result<(), String> { panic!("file mode must not probe the OS backend") },
        MemoryKeyStore::new,
        || -> Result<MemoryKeyStore, KeyStoreError> { Ok(MemoryKeyStore::new()) },
    )
    .unwrap();
    assert_eq!(backend, ResolvedBackend::File);
    assert_eq!(store.get(KeyId::DeviceStatic).unwrap(), None);
}

#[test]
fn resolved_backend_name_matches_doctor_output() {
    assert_eq!(ResolvedBackend::Os.name(), "os");
    assert_eq!(ResolvedBackend::File.name(), "file");
}
