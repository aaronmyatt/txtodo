//! A bounded OS-keychain call (task `relay-id-keystore`): macOS shows a permission prompt for an
//! ad-hoc-signed `txtodod` after every rebuild, and the `keyring` crate blocks until it is
//! answered. Under launchd nobody is at the screen, so an unanswered prompt used to hang the
//! daemon's startup forever with no log line. Every keychain call now runs on its own thread and
//! gives up after [`KEYCHAIN_TIMEOUT`]: the daemon then fails loud (an explicit `--key-store os`,
//! or a read after a successful probe) or falls back to memory with the existing warning (a
//! defaulted `auto`), never a silent fresh in-memory relay id. A timed-out thread stays parked on
//! the prompt; the process either exits on the error or carries on without it.
//!
//! Reads are single-flight per key (task keychain-prompt-loop): a `get` that times out leaves its
//! read pending, and the next `get` of the same key waits on that one instead of starting a
//! second keychain call, so an unanswered prompt is asked once per process, not once per retry.
//! Ref: <https://doc.rust-lang.org/std/sync/mpsc/struct.Receiver.html#method.recv_timeout>
//! Ref: <https://doc.rust-lang.org/std/sync/struct.Condvar.html#method.wait_timeout_while>

use std::collections::HashMap;
use std::sync::mpsc;
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::time::Duration;

use txtodo_sync::{KeyId, KeyStore, KeyStoreError, Secret};

/// How every "did not answer" reason starts, so a caller can tell an unanswered prompt from a real
/// keychain failure ([`is_unanswered`]).
pub(crate) const UNANSWERED: &str = "the OS keychain did not answer";

/// Whether `e` is a keychain call nobody answered (a pending prompt), not a real failure.
pub(crate) fn is_unanswered(e: &KeyStoreError) -> bool {
    matches!(e, KeyStoreError::Backend { reason, .. } if reason.starts_with(UNANSWERED))
}

fn unanswered(what: &str, timeout: Duration) -> String {
    format!(
        "{UNANSWERED} the {what} within {}s (a permission prompt nobody answered?)",
        timeout.as_secs()
    )
}

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
    rx.recv_timeout(timeout)
        .map_err(|_| unanswered(what, timeout))
}

type ReadResult = Result<Option<Secret>, KeyStoreError>;

/// One keychain read still running. Every caller for the same key waits on it.
struct PendingRead {
    result: Mutex<Option<ReadResult>>,
    done: Condvar,
}

impl PendingRead {
    /// The read's result once it lands within `timeout`, else `None` (still unanswered).
    fn wait(&self, timeout: Duration) -> Option<ReadResult> {
        let guard = self.result.lock().unwrap_or_else(PoisonError::into_inner);
        let (guard, _) = self
            .done
            .wait_timeout_while(guard, timeout, |r| r.is_none())
            .unwrap_or_else(PoisonError::into_inner);
        guard.as_ref().map(copy_result)
    }

    fn finish(&self, result: ReadResult) {
        *self.result.lock().unwrap_or_else(PoisonError::into_inner) = Some(result);
        self.done.notify_all();
    }
}

/// `Secret` is not `Clone` on purpose; a waiter gets its own copy of the bytes.
fn copy_result(result: &ReadResult) -> ReadResult {
    match result {
        Ok(secret) => Ok(secret.as_ref().map(|s| Secret::new(s.expose().to_vec()))),
        Err(e) => Err(e.clone()),
    }
}

/// A `KeyStore` whose every call is bounded by `timeout` — wraps the OS backend only; the file
/// and memory backends never block on a human.
pub(crate) struct TimeoutKeyStore<S> {
    inner: Arc<S>,
    timeout: Duration,
    reads: Mutex<HashMap<KeyId, Arc<PendingRead>>>,
}

impl<S> TimeoutKeyStore<S> {
    pub(crate) fn new(inner: S, timeout: Duration) -> TimeoutKeyStore<S> {
        TimeoutKeyStore {
            inner: Arc::new(inner),
            timeout,
            reads: Mutex::new(HashMap::new()),
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

    /// The read of `id` already running, else a new one on its own thread.
    fn read_of(&self, id: KeyId) -> Arc<PendingRead> {
        let mut reads = self.reads.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(pending) = reads.get(&id) {
            return Arc::clone(pending);
        }
        let pending = Arc::new(PendingRead {
            result: Mutex::new(None),
            done: Condvar::new(),
        });
        let (inner, slot) = (Arc::clone(&self.inner), Arc::clone(&pending));
        std::thread::spawn(move || slot.finish(inner.get(id)));
        reads.insert(id, Arc::clone(&pending));
        pending
    }

    /// Drops `pending` from the map once it has answered, so a later read asks the keychain again.
    fn forget(&self, id: KeyId, pending: &Arc<PendingRead>) {
        let mut reads = self.reads.lock().unwrap_or_else(PoisonError::into_inner);
        if reads.get(&id).is_some_and(|p| Arc::ptr_eq(p, pending)) {
            reads.remove(&id);
        }
    }
}

impl<S: KeyStore + Send + Sync + 'static> KeyStore for TimeoutKeyStore<S> {
    fn get(&self, id: KeyId) -> Result<Option<Secret>, KeyStoreError> {
        let pending = self.read_of(id);
        let Some(result) = pending.wait(self.timeout) else {
            return Err(KeyStoreError::Backend {
                id,
                reason: unanswered("read", self.timeout),
            });
        };
        self.forget(id, &pending);
        result
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

    /// Counts keychain reads and answers each after `delay`.
    struct Counting {
        delay: Duration,
        reads: std::sync::atomic::AtomicUsize,
    }

    impl KeyStore for Counting {
        fn get(&self, _id: KeyId) -> Result<Option<Secret>, KeyStoreError> {
            self.reads.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            std::thread::sleep(self.delay);
            Ok(Some(Secret::new(vec![9; 32])))
        }
        fn put(&self, _id: KeyId, _secret: &Secret) -> Result<(), KeyStoreError> {
            Ok(())
        }
        fn delete(&self, _id: KeyId) -> Result<(), KeyStoreError> {
            Ok(())
        }
    }

    #[test]
    fn a_retry_joins_the_pending_read_instead_of_prompting_again() {
        let store = TimeoutKeyStore::new(
            Counting {
                delay: Duration::from_millis(300),
                reads: std::sync::atomic::AtomicUsize::new(0),
            },
            Duration::from_millis(40),
        );
        let first = store.get(KeyId::DeviceStatic).unwrap_err();
        assert!(is_unanswered(&first), "{first}");
        let second = store.get(KeyId::DeviceStatic).unwrap_err();
        assert!(is_unanswered(&second), "{second}");
        std::thread::sleep(Duration::from_millis(400));
        let got = store
            .get(KeyId::DeviceStatic)
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(got.map(|s| s.expose().len()), Some(32));
        let reads = store.inner.reads.load(std::sync::atomic::Ordering::SeqCst);
        assert_eq!(
            reads, 1,
            "one keychain read, so one prompt, for three calls"
        );
    }

    #[test]
    fn a_real_failure_is_not_an_unanswered_prompt() {
        let failed = KeyStoreError::Backend {
            id: KeyId::DeviceStatic,
            reason: "item not accessible".to_owned(),
        };
        assert!(!is_unanswered(&failed));
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
