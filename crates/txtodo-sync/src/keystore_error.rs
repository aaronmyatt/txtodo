//! One typed error for every keystore path. Never carries key bytes (CLAUDE.md §3.1): every variant
//! names the `KeyId` and the backend, nothing else.

use std::fmt;
use std::path::PathBuf;

use crate::keystore::KeyId;

/// Why a keystore read, write or backend selection failed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum KeyStoreError {
    /// The requested backend cannot be used at all (no OS keystore session, e.g. headless Linux
    /// with no D-Bus secret service).
    Unavailable {
        /// Backend that was asked for (`"os"` or `"file"`).
        backend: &'static str,
        /// Human-readable reason, from the underlying library.
        reason: String,
    },
    /// `key_store = "auto"` could not find a usable OS backend. Per the task: this must never
    /// fall back to a file silently — the caller stops and tells the human what to do.
    AutoNeedsChoice {
        /// Why the OS backend was rejected.
        reason: String,
    },
    /// The encrypted-file backend's file has group- or world-readable permissions. Refused, never
    /// repaired: a permissive file may already have been read.
    PermissionsTooOpen {
        /// The file in question.
        path: PathBuf,
        /// The mode bits actually found.
        mode: u32,
    },
    /// The file's header does not parse (wrong magic, unsupported version, or truncated).
    Corrupt {
        /// The file in question.
        path: PathBuf,
        /// What was wrong.
        reason: String,
    },
    /// The AEAD tag did not verify: wrong passphrase, or the file was tampered with.
    WrongPassphrase {
        /// The file in question.
        path: PathBuf,
    },
    /// A filesystem operation failed (create, read, write, set-permissions).
    Io {
        /// The file in question.
        path: PathBuf,
        /// The OS error text.
        reason: String,
    },
    /// The OS CSPRNG could not produce a salt or nonce.
    Entropy,
    /// The keystore's own serialization of its entry map failed.
    Encode(String),
    /// The underlying OS keystore backend reported an error for one entry (get/put/delete).
    Backend {
        /// Which key was being accessed.
        id: KeyId,
        /// The backend's error text.
        reason: String,
    },
}

impl fmt::Display for KeyStoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            KeyStoreError::Unavailable { backend, reason } => {
                write!(f, "{backend} keystore backend is unavailable: {reason}")
            }
            KeyStoreError::AutoNeedsChoice { reason } => write!(
                f,
                "no OS keystore is available ({reason}); set key_store = \"file\" to use an \
                 encrypted file, or fix the OS keystore and set key_store = \"os\""
            ),
            KeyStoreError::PermissionsTooOpen { path, mode } => write!(
                f,
                "{} is group- or world-readable (mode {mode:o}); refusing to open a key file that \
                 may already have been read",
                path.display()
            ),
            KeyStoreError::Corrupt { path, reason } => {
                write!(
                    f,
                    "{} is not a valid keystore file: {reason}",
                    path.display()
                )
            }
            KeyStoreError::WrongPassphrase { path } => {
                write!(
                    f,
                    "wrong passphrase, or {} was tampered with",
                    path.display()
                )
            }
            KeyStoreError::Io { path, reason } => {
                write!(f, "I/O error on {}: {reason}", path.display())
            }
            KeyStoreError::Entropy => {
                write!(f, "the OS CSPRNG failed; no salt or nonce was produced")
            }
            KeyStoreError::Encode(reason) => write!(f, "cannot encode keystore entries: {reason}"),
            KeyStoreError::Backend { id, reason } => {
                write!(f, "OS keystore backend failed for {id:?}: {reason}")
            }
        }
    }
}

impl std::error::Error for KeyStoreError {}
