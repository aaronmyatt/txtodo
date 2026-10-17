//! The vault (task can-we-limit-the-number-of-keychain-pass) over a counting in-memory store: one
//! inner read however many keys are asked for, old items moved in once and deleted, writes kept
//! when the item cannot be written, and a vault that does not decode never overwritten.

use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use super::VaultKeyStore;
use crate::keystore::{KeyId, KeyStore, Secret};
use crate::keystore_error::KeyStoreError;
use crate::keystore_memory::MemoryKeyStore;

/// A memory store that counts reads of items that exist (each one a prompt on the real keychain)
/// and can refuse writes.
#[derive(Default)]
struct Counting {
    inner: MemoryKeyStore,
    reads: AtomicUsize,
    refuse_puts: Mutex<bool>,
}

impl KeyStore for Counting {
    fn get(&self, id: KeyId) -> Result<Option<Secret>, KeyStoreError> {
        let got = self.inner.get(id)?;
        if got.is_some() {
            self.reads.fetch_add(1, Ordering::SeqCst);
        }
        Ok(got)
    }
    fn put(&self, id: KeyId, secret: &Secret) -> Result<(), KeyStoreError> {
        if *self.refuse_puts.lock().unwrap_or_else(|e| e.into_inner()) {
            let reason = "refused".to_owned();
            return Err(KeyStoreError::Backend { id, reason });
        }
        self.inner.put(id, secret)
    }
    fn delete(&self, id: KeyId) -> Result<(), KeyStoreError> {
        self.inner.delete(id)
    }
}

fn s(b: u8) -> Secret {
    Secret::new(vec![b; 4])
}

fn get(store: &impl KeyStore, id: KeyId) -> Option<Vec<u8>> {
    let got = store.get(id).unwrap_or_else(|e| panic!("get {id}: {e}"));
    got.map(|s| s.expose().to_vec())
}

#[test]
fn every_key_lives_in_one_item_read_once() {
    let vault = VaultKeyStore::new(Counting::default());
    for (i, id) in [KeyId::DeviceStatic, KeyId::RelayIdentity, KeyId::Group(0)]
        .into_iter()
        .enumerate()
    {
        vault.put(id, &s(i as u8)).unwrap_or_else(|e| panic!("{e}"));
    }
    assert_eq!(
        get(&vault.inner.inner, KeyId::DeviceStatic),
        None,
        "no item of its own"
    );

    // A new process: one read of the vault item answers every key.
    let reopened = VaultKeyStore::new(Counting {
        inner: MemoryKeyStore::new(),
        ..Counting::default()
    });
    let item = vault
        .inner
        .inner
        .get(KeyId::Vault)
        .unwrap_or_else(|e| panic!("{e}"));
    let item = item.unwrap_or_else(|| panic!("the vault item"));
    reopened
        .inner
        .inner
        .put(KeyId::Vault, &item)
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(get(&reopened, KeyId::Group(0)), Some(vec![2; 4]));
    assert_eq!(get(&reopened, KeyId::DeviceStatic), Some(vec![0; 4]));
    assert_eq!(get(&reopened, KeyId::DeviceSigning), None);
    assert_eq!(
        reopened.inner.reads.load(Ordering::SeqCst),
        1,
        "one keychain read"
    );
}

#[test]
fn an_old_item_moves_in_once_and_is_deleted() {
    let old = Counting::default();
    old.inner
        .put(KeyId::DeviceStatic, &s(9))
        .unwrap_or_else(|e| panic!("{e}"));
    let vault = VaultKeyStore::new(old);
    assert_eq!(get(&vault, KeyId::DeviceStatic), Some(vec![9; 4]));
    assert_eq!(
        get(&vault.inner.inner, KeyId::DeviceStatic),
        None,
        "the old item is gone"
    );
    assert_eq!(
        get(&vault, KeyId::DeviceStatic),
        Some(vec![9; 4]),
        "now from the vault"
    );
    assert!(get(&vault.inner.inner, KeyId::Vault).is_some());
}

#[test]
fn a_refused_write_changes_nothing_in_memory_either() {
    let vault = VaultKeyStore::new(Counting::default());
    vault
        .put(KeyId::Group(1), &s(1))
        .unwrap_or_else(|e| panic!("{e}"));
    *vault
        .inner
        .refuse_puts
        .lock()
        .unwrap_or_else(|e| e.into_inner()) = true;
    assert!(vault.put(KeyId::Group(1), &s(2)).is_err());
    assert!(vault.put(KeyId::Group(2), &s(2)).is_err());
    assert!(vault.delete(KeyId::Group(1)).is_err());
    assert_eq!(get(&vault, KeyId::Group(1)), Some(vec![1; 4]));
    assert_eq!(get(&vault, KeyId::Group(2)), None);
}

#[test]
fn a_vault_that_does_not_decode_is_an_error_and_is_kept() {
    let inner = Counting::default();
    let junk = Secret::new(vec![7, 1, 2, 3]);
    inner
        .inner
        .put(KeyId::Vault, &junk)
        .unwrap_or_else(|e| panic!("{e}"));
    let vault = VaultKeyStore::new(inner);
    assert!(vault.get(KeyId::DeviceStatic).is_err());
    assert!(vault.put(KeyId::DeviceStatic, &s(1)).is_err());
    let kept = get(&vault.inner.inner, KeyId::Vault);
    assert_eq!(kept, Some(vec![7, 1, 2, 3]), "never overwritten");
}
