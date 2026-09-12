//! One typed error for group-key rotation. Never carries key bytes (CLAUDE.md §3.1).

use std::fmt;

use txtodo_model::DeviceId;

/// Why a rotation grant could not be sealed, opened, or planned.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum RotationError {
    /// HKDF derivation failed (unreachable in practice; modelled rather than assumed infallible).
    Derive,
    /// The OS CSPRNG could not produce a nonce.
    Entropy,
    /// XChaCha20-Poly1305 refused to seal the grant.
    Seal,
    /// The AEAD tag did not verify: wrong recipient key, tampered grant, or wrong epoch.
    Open,
    /// A rotation plan named no remaining devices — a group key with nobody to read it.
    NoRemainingDevices,
    /// The epoch counter is already at `u32::MAX`; one more rotation would wrap around.
    EpochOverflow,
}

impl fmt::Display for RotationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RotationError::Derive => {
                write!(f, "HKDF-SHA256 refused to expand to the requested length")
            }
            RotationError::Entropy => write!(f, "the OS CSPRNG failed; no nonce was produced"),
            RotationError::Seal => write!(f, "XChaCha20-Poly1305 refused to seal the grant"),
            RotationError::Open => {
                write!(f, "grant did not open: wrong key, tampered, or wrong epoch")
            }
            RotationError::NoRemainingDevices => {
                write!(
                    f,
                    "rotation refused: no remaining devices to grant the new key to"
                )
            }
            RotationError::EpochOverflow => write!(f, "epoch counter would overflow u32::MAX"),
        }
    }
}

impl std::error::Error for RotationError {}

/// Why `txtodo device remove` refused, before any crypto runs.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum RemovalError {
    /// A device may not remove itself; that is "leave the group", a different flow.
    CannotRemoveSelf {
        /// The device that tried.
        device: DeviceId,
    },
    /// A group with one device left has nobody to hand a rotated key to.
    CannotRemoveLastDevice,
}

impl fmt::Display for RemovalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RemovalError::CannotRemoveSelf { device } => {
                write!(
                    f,
                    "device {device} cannot remove itself; that is leaving the group"
                )
            }
            RemovalError::CannotRemoveLastDevice => {
                write!(f, "cannot remove the last device in the group")
            }
        }
    }
}

impl std::error::Error for RemovalError {}
