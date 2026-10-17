//! Every key in one OS keychain item (task can-we-limit-the-number-of-keychain-pass, decided
//! 2026-10-17). On macOS each item has its own access list, so a new code signature on `txtodod`
//! used to cost one prompt per item: up to four, at different times. The vault is one item,
//! [`KeyId::Vault`], read once and kept in memory, so it costs one.
//!
//! - The first call reads the vault, under a lock: two keys asked at once still make one read.
//! - `put`/`delete` change the map and write the whole item back.
//! - Migration is lazy: a key the vault lacks is read from its own old item once, copied in, and
//!   the old item deleted. After that only the vault is read.
//! - A vault that does not decode is an error, never overwritten.
//!
//! Format: one version byte, then postcard of `Vec<(KeyId, Vec<u8>)>`.
//! Ref: <https://docs.rs/postcard>

use std::collections::BTreeMap;
use std::sync::{Mutex, MutexGuard, PoisonError};

use zeroize::Zeroize;

use crate::keystore::{KeyId, KeyStore, Secret};
use crate::keystore_error::KeyStoreError;

/// The vault format this build reads and writes.
const VAULT_VERSION: u8 = 1;

/// [`KeyStore`] that keeps every key in one item of `inner` ([`KeyId::Vault`]). `inner` is the OS
/// keystore in production; any store works, which is how the tests run.
pub struct VaultKeyStore<S> {
    inner: S,
    /// `None` until the first call reads the vault.
    keys: Mutex<Option<BTreeMap<KeyId, Secret>>>,
}

impl<S: KeyStore> VaultKeyStore<S> {
    /// Wraps `inner`, touching nothing yet.
    pub fn new(inner: S) -> VaultKeyStore<S> {
        VaultKeyStore {
            inner,
            keys: Mutex::new(None),
        }
    }

    /// The map, read from the vault item on the first call. The guard is held across that read.
    fn loaded(&self) -> Result<MutexGuard<'_, Option<BTreeMap<KeyId, Secret>>>, KeyStoreError> {
        let mut keys = self.keys.lock().unwrap_or_else(PoisonError::into_inner);
        if keys.is_none() {
            *keys = Some(match self.inner.get(KeyId::Vault)? {
                Some(bytes) => decode(bytes.expose())?,
                None => BTreeMap::new(),
            });
        }
        Ok(keys)
    }

    /// Writes `keys` as the vault item.
    fn save(&self, keys: &BTreeMap<KeyId, Secret>) -> Result<(), KeyStoreError> {
        self.inner.put(KeyId::Vault, &encode(keys)?)
    }
}

impl<S: KeyStore> KeyStore for VaultKeyStore<S> {
    fn get(&self, id: KeyId) -> Result<Option<Secret>, KeyStoreError> {
        let mut guard = self.loaded()?;
        let keys = guard.get_or_insert_with(BTreeMap::new);
        if let Some(secret) = keys.get(&id) {
            return Ok(Some(secret.clone()));
        }
        // Not moved in yet: its own old item, once. A key that never existed reads `None` here and
        // costs no prompt (no item, no access list).
        let Some(old) = self.inner.get(id)? else {
            return Ok(None);
        };
        keys.insert(id, old.clone());
        self.save(keys)?;
        // Best effort: an old item left behind is only read again if the vault loses the key.
        let _ = self.inner.delete(id);
        Ok(Some(old))
    }

    fn put(&self, id: KeyId, secret: &Secret) -> Result<(), KeyStoreError> {
        let mut guard = self.loaded()?;
        let keys = guard.get_or_insert_with(BTreeMap::new);
        let before = keys.insert(id, secret.clone());
        if let Err(e) = self.save(keys) {
            // The item still holds the old map: so must memory.
            match before {
                Some(old) => keys.insert(id, old),
                None => keys.remove(&id),
            };
            return Err(e);
        }
        Ok(())
    }

    fn delete(&self, id: KeyId) -> Result<(), KeyStoreError> {
        let mut guard = self.loaded()?;
        let keys = guard.get_or_insert_with(BTreeMap::new);
        if let Some(old) = keys.remove(&id)
            && let Err(e) = self.save(keys)
        {
            keys.insert(id, old);
            return Err(e);
        }
        // An old item of this key, if it never moved in, goes too.
        self.inner.delete(id)
    }
}

fn encode(keys: &BTreeMap<KeyId, Secret>) -> Result<Secret, KeyStoreError> {
    let mut pairs: Vec<(KeyId, Vec<u8>)> = keys
        .iter()
        .map(|(id, s)| (*id, s.expose().to_vec()))
        .collect();
    let body = postcard::to_allocvec(&pairs).map_err(|e| KeyStoreError::Encode(e.to_string()));
    pairs.iter_mut().for_each(|(_, bytes)| bytes.zeroize());
    let mut body = body?;
    let mut out = Vec::with_capacity(body.len() + 1);
    out.push(VAULT_VERSION);
    out.extend_from_slice(&body);
    body.zeroize();
    Ok(Secret::new(out))
}

fn decode(bytes: &[u8]) -> Result<BTreeMap<KeyId, Secret>, KeyStoreError> {
    let corrupt = |reason: String| KeyStoreError::Backend {
        id: KeyId::Vault,
        reason,
    };
    let Some((&version, body)) = bytes.split_first() else {
        return Err(corrupt("the vault item is empty".to_owned()));
    };
    if version != VAULT_VERSION {
        return Err(corrupt(format!(
            "vault version {version}, this build reads {VAULT_VERSION}"
        )));
    }
    let pairs: Vec<(KeyId, Vec<u8>)> = postcard::from_bytes(body)
        .map_err(|e| corrupt(format!("the vault does not decode: {e}")))?;
    Ok(pairs
        .into_iter()
        .filter(|(id, _)| *id != KeyId::Vault)
        .map(|(id, bytes)| (id, Secret::new(bytes)))
        .collect())
}

#[cfg(test)]
#[path = "keystore_vault_tests.rs"]
mod tests;
