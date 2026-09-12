//! One typed error for the pairing state machine. Every failure names the phase it was refused in
//! rather than a generic "pairing failed" — a human debugging a stuck pairing needs to know whether
//! the nonce was the problem or the confirmation was.

use std::fmt;

use crate::nonce_registry::NonceError;
use crate::sas::SasError;

/// Why a pairing step was refused.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum PairingError {
    /// The offer's nonce failed the registry's checks.
    Nonce(NonceError),
    /// HKDF derivation failed (see [`SasError`]; unreachable in practice).
    Derive(SasError),
    /// A step that needs the shared secret / SAS ran before the handshake computed one.
    NotHandshaken,
    /// The group key was requested before both sides confirmed the SAS.
    NotConfirmed,
    /// This pairing window is closed — either the SAS was rejected, mismatched confirmations were
    /// exhausted, or the key was already sent. No further step is accepted.
    Closed,
    /// The AEAD wrap/unwrap of the group key failed (wrong key, tampered ciphertext, or the pairing
    /// was not actually confirmed on the peer's side despite claiming so).
    Seal,
}

impl fmt::Display for PairingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PairingError::Nonce(e) => write!(f, "pairing nonce refused: {e:?}"),
            PairingError::Derive(e) => write!(f, "pairing key derivation failed: {e}"),
            PairingError::NotHandshaken => {
                write!(f, "no shared secret yet: the handshake has not completed")
            }
            PairingError::NotConfirmed => {
                write!(
                    f,
                    "the group key cannot move until both sides confirm the SAS"
                )
            }
            PairingError::Closed => write!(f, "this pairing window is closed"),
            PairingError::Seal => write!(f, "sealing or opening the group key transfer failed"),
        }
    }
}

impl std::error::Error for PairingError {}

impl From<NonceError> for PairingError {
    fn from(e: NonceError) -> PairingError {
        PairingError::Nonce(e)
    }
}

impl From<SasError> for PairingError {
    fn from(e: SasError) -> PairingError {
        PairingError::Derive(e)
    }
}
