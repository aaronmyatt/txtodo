//! OS keystore backend: macOS Keychain, Windows Credential Manager, Linux Secret Service over
//! D-Bus. Ref: <https://docs.rs/keyring>. No test in this crate touches the real backend — that
//! would pop a Keychain dialog in CI — so every test here runs against [`crate::keystore_memory`]
//! or a fake probe instead.

use keyring::Entry;

use crate::keystore::{KeyId, KeyStore, Secret};
use crate::keystore_error::KeyStoreError;

/// One OS keystore, scoped to `scope` (typically the workspace or group id) so two todo groups on
/// one machine never share an entry.
pub struct OsKeyStore {
    scope: String,
}

fn entry(scope: &str, id: KeyId) -> Result<Entry, KeyStoreError> {
    Entry::new("txtodo", &format!("{scope}/{id}")).map_err(|e| KeyStoreError::Backend {
        id,
        reason: e.to_string(),
    })
}

impl OsKeyStore {
    /// Opens the OS backend for `scope`, without touching any entry yet.
    pub fn new(scope: impl Into<String>) -> OsKeyStore {
        OsKeyStore {
            scope: scope.into(),
        }
    }

    /// Checks that the OS backend can actually be reached, by round-tripping a throwaway entry.
    /// This is the check `key_store = "auto"` runs before trusting the backend
    /// ([`crate::keystore_resolve`]) — real I/O, but touching only a probe entry it deletes again.
    pub fn probe(scope: &str) -> Result<(), String> {
        let e = Entry::new("txtodo", &format!("{scope}/probe")).map_err(|e| e.to_string())?;
        e.set_password("probe").map_err(|e| e.to_string())?;
        let readback = e.get_password().map_err(|e| e.to_string())?;
        let _ = e.delete_credential();
        if readback != "probe" {
            return Err("probe entry read back a different value".to_string());
        }
        Ok(())
    }
}

impl KeyStore for OsKeyStore {
    fn get(&self, id: KeyId) -> Result<Option<Secret>, KeyStoreError> {
        match entry(&self.scope, id)?.get_secret() {
            Ok(bytes) => Ok(Some(Secret::new(bytes))),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(KeyStoreError::Backend {
                id,
                reason: e.to_string(),
            }),
        }
    }

    fn put(&self, id: KeyId, secret: &Secret) -> Result<(), KeyStoreError> {
        entry(&self.scope, id)?
            .set_secret(secret.expose())
            .map_err(|e| KeyStoreError::Backend {
                id,
                reason: e.to_string(),
            })
    }

    fn delete(&self, id: KeyId) -> Result<(), KeyStoreError> {
        match entry(&self.scope, id)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(KeyStoreError::Backend {
                id,
                reason: e.to_string(),
            }),
        }
    }
}
