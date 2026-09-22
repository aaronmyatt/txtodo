//! A bounded OS-keychain call (task `relay-id-keystore`): macOS shows a permission prompt for an
//! ad-hoc-signed `txtodod` after every rebuild, and the `keyring` crate blocks until it is
//! answered. Under launchd nobody is at the screen, so an unanswered prompt used to hang the
//! daemon's startup forever with no log line. Every keychain call now runs on its own thread and
//! gives up after [`KEYCHAIN_TIMEOUT`]: the daemon then fails loud (an explicit `--key-store os`,
//! or a read after a successful probe) or falls back to memory with the existing warning (a
//! defaulted `auto`), never a silent fresh in-memory relay id. A timed-out thread stays parked on
//! the prompt; the process either exits on the error or carries on without it.
//! Ref: <https://doc.rust-lang.org/std/sync/mpsc/struct.Receiver.html#method.recv_timeout>

use std::sync::Arc;
use std::sync::mpsc;
use std::time::Duration;

use txtodo_sync::{KeyId, KeyStore, KeyStoreError, Secret};

/// How long one keychain call may take before it counts as unanswered. A real keychain answers
/// in milliseconds; a prompt sits there until a human clicks.
pub(crate) const KEYCHAIN_TIMEOUT: Duration = Duration::from_secs(20);

/// Runs `f` on its own thread and waits at most `timeout` for its result. `Err` names what did
/// not answer; the thread is left running.
pub(crate) fn bounded<T: Send + 'static>(
    what: &'static str,
    timeout: Duration,
    f: impl FnOnce() -> T + Send + 'static,
) -> Result<T, String> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(f());
    });
    rx.recv_timeout(timeout).map_err(|_| {
        format!(
            "the OS keychain did not answer the {what} within {}s (a permission prompt nobody \
             answered?)",
            timeout.as_secs()
        )
    })
}

/// A `KeyStore` whose every call is bounded by `timeout` — wraps the OS backend only; the file
/// and memory backends never block on a human.
pub(crate) struct TimeoutKeyStore<S> {
    inner: Arc<S>,
    timeout: Duration,
}

impl<S> TimeoutKeyStore<S> {
    pub(crate) fn new(inner: S, timeout: Duration) -> TimeoutKeyStore<S> {
        TimeoutKeyStore {
            inner: Arc::new(inner),
            timeout,
        }
    }
}

impl<S: KeyStore + Send + Sync + 'static> TimeoutKeyStore<S> {
    fn call<T: Send + 'static>(
        &self,
        what: &'static str,
        id: KeyId,
        f: impl FnOnce(&S) -> Result<T, KeyStoreError> + Send + 'static,
    ) -> Result<T, KeyStoreError> {
        let inner = Arc::clone(&self.inner);
        match bounded(what, self.timeout, move || f(&inner)) {
            Ok(result) => result,
            Err(reason) => Err(KeyStoreError::Backend { id, reason }),
        }
    }
}

impl<S: KeyStore + Send + Sync + 'static> KeyStore for TimeoutKeyStore<S> {
    fn get(&self, id: KeyId) -> Result<Option<Secret>, KeyStoreError> {
        self.call("read", id, move |s| s.get(id))
    }

    fn put(&self, id: KeyId, secret: &Secret) -> Result<(), KeyStoreError> {
        let secret = Secret::new(secret.expose().to_vec());
        self.call("write", id, move |s| s.put(id, &secret))
    }

    fn delete(&self, id: KeyId) -> Result<(), KeyStoreError> {
        self.call("delete", id, move |s| s.delete(id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A keystore that answers only after `delay` — a stand-in for a prompt nobody clicks.
    struct Slow {
        delay: Duration,
    }

    impl KeyStore for Slow {
        fn get(&self, _id: KeyId) -> Result<Option<Secret>, KeyStoreError> {
            std::thread::sleep(self.delay);
            Ok(Some(Secret::new(vec![7; 32])))
        }
        fn put(&self, _id: KeyId, _secret: &Secret) -> Result<(), KeyStoreError> {
            std::thread::sleep(self.delay);
            Ok(())
        }
        fn delete(&self, _id: KeyId) -> Result<(), KeyStoreError> {
            Ok(())
        }
    }

    #[test]
    fn an_unanswered_read_fails_loud_instead_of_hanging() {
        let store = TimeoutKeyStore::new(
            Slow {
                delay: Duration::from_millis(400),
            },
            Duration::from_millis(50),
        );
        let started = std::time::Instant::now();
        let err = store.get(KeyId::RelayIdentity).unwrap_err();
        assert!(
            started.elapsed() < Duration::from_millis(350),
            "gave up early"
        );
        let text = err.to_string();
        assert!(text.contains("did not answer the read"), "{text}");
        assert!(
            store
                .put(KeyId::RelayIdentity, &Secret::new(vec![1]))
                .is_err()
        );
    }

    #[test]
    fn an_answered_read_passes_through() {
        let store = TimeoutKeyStore::new(
            Slow {
                delay: Duration::from_millis(10),
            },
            Duration::from_secs(2),
        );
        let got = store
            .get(KeyId::DeviceStatic)
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(got.map(|s| s.expose().len()), Some(32));
        assert!(store.delete(KeyId::DeviceStatic).is_ok());
    }

    #[test]
    fn bounded_reports_what_timed_out() {
        let err = bounded("probe", Duration::from_millis(20), || {
            std::thread::sleep(Duration::from_millis(200));
        })
        .unwrap_err();
        assert!(err.contains("probe"), "{err}");
        assert_eq!(bounded("probe", Duration::from_secs(1), || 5), Ok(5));
    }
}
