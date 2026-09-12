//! In-memory `KeyStore`, for tests. No test ever touches a real OS keystore or an encrypted file
//! (CLAUDE.md: no dialog popping up in CI, no temp file to clean up).

use std::collections::BTreeMap;
use std::sync::{Mutex, PoisonError};

use crate::keystore::{KeyId, KeyStore, Secret};
use crate::keystore_error::KeyStoreError;

/// Holds secrets in a `Mutex<BTreeMap>` for the lifetime of the process; nothing is persisted.
#[derive(Default)]
pub struct MemoryKeyStore {
    entries: Mutex<BTreeMap<KeyId, Secret>>,
}

impl MemoryKeyStore {
    /// An empty store.
    pub fn new() -> MemoryKeyStore {
        MemoryKeyStore::default()
    }
}

impl KeyStore for MemoryKeyStore {
    fn get(&self, id: KeyId) -> Result<Option<Secret>, KeyStoreError> {
        // A poisoned lock means a prior panic; the map itself is still consistent (plain insert/
        // remove, no partial writes), so keep going with the inner value rather than panic again.
        let entries = self.entries.lock().unwrap_or_else(PoisonError::into_inner);
        Ok(entries.get(&id).cloned())
    }

    fn put(&self, id: KeyId, secret: &Secret) -> Result<(), KeyStoreError> {
        let mut entries = self.entries.lock().unwrap_or_else(PoisonError::into_inner);
        let before = entries.len();
        entries.insert(id, secret.clone());
        debug_assert!(entries.len() == before || entries.len() == before + 1);
        debug_assert!(entries.contains_key(&id));
        Ok(())
    }

    fn delete(&self, id: KeyId) -> Result<(), KeyStoreError> {
        let mut entries = self.entries.lock().unwrap_or_else(PoisonError::into_inner);
        entries.remove(&id);
        debug_assert!(!entries.contains_key(&id));
        Ok(())
    }
}
