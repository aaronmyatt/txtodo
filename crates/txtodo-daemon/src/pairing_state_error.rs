//! [`PairingStateError`], split out of `pairing_state.rs` for its file budget — the same
//! `*_error.rs` pattern `workspace_error.rs` and `txtodo-sync`'s own error files already use.

use txtodo_sync::{
    KEY_BYTES, KeyStoreError, MAX_CONCURRENT_PAIRINGS, PAIRING_WINDOW_MS, PairingError,
};

/// Why a pairing step was refused: this daemon's own bookkeeping folded in with
/// [`PairingError`] so `pairing_grpc.rs` has one error type to map to a `Status`.
#[derive(Debug)]
pub(crate) enum PairingStateError {
    /// `MAX_CONCURRENT_PAIRINGS` already active and not expired.
    TooManyOpen,
    /// The active pairing's `PAIRING_WINDOW_MS` window elapsed; it has been cleared.
    WindowExpired,
    /// No pairing is active on this daemon.
    NotActive,
    /// The active pairing exists but is not in the role this call needs.
    WrongRole,
    /// The state machine itself refused the step.
    Session(PairingError),
    /// The keystore refused a read or write.
    KeyStore(KeyStoreError),
    /// Persisting the adopted group id to the store's `meta` table failed.
    Store(txtodo_store::StoreError),
    /// The stored group key is not `KEY_BYTES` long (the keystore was edited or corrupted by hand).
    CorruptGroupKey(usize),
}

impl std::fmt::Display for PairingStateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PairingStateError::TooManyOpen => {
                write!(
                    f,
                    "{MAX_CONCURRENT_PAIRINGS} pairing(s) already open on this daemon"
                )
            }
            PairingStateError::WindowExpired => {
                write!(f, "the pairing window ({PAIRING_WINDOW_MS} ms) has expired")
            }
            PairingStateError::NotActive => write!(f, "no pairing is active on this daemon"),
            PairingStateError::WrongRole => {
                write!(f, "the active pairing is not in the role this call needs")
            }
            PairingStateError::Session(e) => write!(f, "{e}"),
            PairingStateError::KeyStore(e) => write!(f, "{e}"),
            PairingStateError::Store(e) => write!(f, "{e}"),
            PairingStateError::CorruptGroupKey(len) => {
                write!(f, "stored group key is {len} bytes, not {KEY_BYTES}")
            }
        }
    }
}

impl std::error::Error for PairingStateError {}

impl From<PairingError> for PairingStateError {
    fn from(e: PairingError) -> PairingStateError {
        PairingStateError::Session(e)
    }
}

impl From<KeyStoreError> for PairingStateError {
    fn from(e: KeyStoreError) -> PairingStateError {
        PairingStateError::KeyStore(e)
    }
}

impl From<txtodo_store::StoreError> for PairingStateError {
    fn from(e: txtodo_store::StoreError) -> PairingStateError {
        PairingStateError::Store(e)
    }
}
