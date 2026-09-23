//! An in-memory read cache in front of the OS keystore: one keychain touch per key per daemon
//! boot instead of one per sync session.
//!
//! Every LAN, relay and control session used to call `key_store().get(KeyId::Group(..))`
//! (`lan_session.rs`, `control_session.rs`). With the resync dialer firing every second on both
//! peers that was ~3 keychain reads a second; on an ad-hoc-signed `txtodod` every read is a macOS
//! permission dialog, and `keystore_timeout.rs` leaves one thread parked per unanswered prompt.
//! Observed 2026-09-23 as a modal storm on both Macs that blocked the pairing SAS confirm until the
//! 120 s window expired.
//!
//! Semantics: a successful `get` (hit or miss) is remembered; `put`/`delete` write through and
//! update the entry; errors are never cached. Caching the miss is deliberate: before pairing lands
//! the group key does not exist yet, and that read is the one every session repeats. It is safe
//! only because this daemon is the sole writer of its own keys while it runs (one
//! `resolve_key_store` per process, `device_identity.rs`), so every write goes through this wrapper.
//! Ref: <https://doc.rust-lang.org/std/collections/struct.HashMap.html>

use std::collections::HashMap;
use std::sync::{Mutex, PoisonError};

use txtodo_sync::{KeyId, KeyStore, KeyStoreError, Secret};

/// [`KeyStore`] decorator that answers repeat reads from memory. Wraps the OS backend only; the
/// file and memory backends are already cheap.
pub(crate) struct CachedKeyStore<S> {
    inner: S,
    cache: Mutex<HashMap<KeyId, Option<Secret>>>,
}

impl<S> CachedKeyStore<S> {
    pub(crate) fn new(inner: S) -> CachedKeyStore<S> {
        CachedKeyStore {
            inner,
            cache: Mutex::new(HashMap::new()),
        }
    }

    fn cached(&self, id: KeyId) -> Option<Option<Secret>> {
        self.cache
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&id)
            .cloned()
    }

    fn remember(&self, id: KeyId, value: Option<Secret>) {
        self.cache
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(id, value);
    }
}

impl<S: KeyStore> KeyStore for CachedKeyStore<S> {
    fn get(&self, id: KeyId) -> Result<Option<Secret>, KeyStoreError> {
        if let Some(hit) = self.cached(id) {
            return Ok(hit);
        }
        let fetched = self.inner.get(id)?;
        self.remember(id, fetched.clone());
        Ok(fetched)
    }

    fn put(&self, id: KeyId, secret: &Secret) -> Result<(), KeyStoreError> {
        self.inner.put(id, secret)?;
        self.remember(id, Some(secret.clone()));
        Ok(())
    }

    fn delete(&self, id: KeyId) -> Result<(), KeyStoreError> {
        self.inner.delete(id)?;
        self.remember(id, None);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// A keystore that counts how often the backend is actually asked.
    struct Counting {
        reads: AtomicUsize,
        inner: txtodo_sync::MemoryKeyStore,
        fail: bool,
    }

    impl Counting {
        fn new(fail: bool) -> Counting {
            Counting {
                reads: AtomicUsize::new(0),
                inner: txtodo_sync::MemoryKeyStore::default(),
                fail,
            }
        }
    }

    impl KeyStore for Counting {
        fn get(&self, id: KeyId) -> Result<Option<Secret>, KeyStoreError> {
            self.reads.fetch_add(1, Ordering::SeqCst);
            if self.fail {
                return Err(KeyStoreError::Backend {
                    id,
                    reason: "prompt unanswered".to_owned(),
                });
            }
            self.inner.get(id)
        }
        fn put(&self, id: KeyId, secret: &Secret) -> Result<(), KeyStoreError> {
            self.inner.put(id, secret)
        }
        fn delete(&self, id: KeyId) -> Result<(), KeyStoreError> {
            self.inner.delete(id)
        }
    }

    fn reads(store: &CachedKeyStore<Counting>) -> usize {
        store.inner.reads.load(Ordering::SeqCst)
    }

    #[test]
    fn a_miss_is_read_once_then_answered_from_memory() {
        let store = CachedKeyStore::new(Counting::new(false));
        for _ in 0..5 {
            assert!(store.get(KeyId::Group(0)).unwrap().is_none());
        }
        assert_eq!(reads(&store), 1, "the backend was asked once");
    }

    #[test]
    fn a_write_updates_the_cached_entry_without_a_reread() {
        let store = CachedKeyStore::new(Counting::new(false));
        assert!(store.get(KeyId::Group(0)).unwrap().is_none());
        store
            .put(KeyId::Group(0), &Secret::new(vec![9; 32]))
            .unwrap();
        let got = store.get(KeyId::Group(0)).unwrap();
        assert_eq!(got.map(|s| s.expose().to_vec()), Some(vec![9; 32]));
        assert_eq!(reads(&store), 1, "put refreshed the cache; no reread");
        store.delete(KeyId::Group(0)).unwrap();
        assert!(store.get(KeyId::Group(0)).unwrap().is_none());
        assert_eq!(reads(&store), 1);
    }

    #[test]
    fn a_backend_error_is_not_cached() {
        let store = CachedKeyStore::new(Counting::new(true));
        assert!(store.get(KeyId::DeviceStatic).is_err());
        assert!(store.get(KeyId::DeviceStatic).is_err());
        assert_eq!(reads(&store), 2, "each failed read reaches the backend");
    }

    #[test]
    fn entries_are_per_key() {
        let store = CachedKeyStore::new(Counting::new(false));
        store
            .put(KeyId::Group(1), &Secret::new(vec![1; 32]))
            .unwrap();
        assert!(store.get(KeyId::Group(1)).unwrap().is_some());
        assert!(store.get(KeyId::Group(2)).unwrap().is_none());
        assert_eq!(
            reads(&store),
            1,
            "only the unknown epoch reached the backend"
        );
    }
}
