//! Device signatures over ops. A signature answers "who wrote this op", per op, forever: Ed25519
//! over `Op::signing_bytes`, stored in the durable `ops.signature` column and re-verifiable during
//! `blame` years later. It is not a transport property — that is why there is no sign-then-encrypt
//! argument to win: a re-encrypted batch still carries each op's original author signature.
//!
//! The device key is **injected**, never read from a file, env var or the OS keystore here. That
//! keeps this module free of I/O and lets a test pass a fixture key. Keys come from
//! `tasks/sync-keystore`.
//!
//! RustCrypto ed25519 API: <https://docs.rs/ed25519-dalek> · RFC 8032
//! <https://www.rfc-editor.org/rfc/rfc8032>.

use std::collections::BTreeMap;
use std::fmt;

use ed25519_dalek::{Signer, SigningKey, VerifyingKey};
use serde::{Deserialize, Serialize};
use txtodo_model::{DeviceId, Op};

use crate::crypto_error::CryptoError;

/// Bytes in an Ed25519 seed / signing key.
pub const SIGNING_KEY_BYTES: usize = 32;
/// Bytes in an Ed25519 compressed public key.
pub const PUBLIC_KEY_BYTES: usize = 32;
/// Bytes in an Ed25519 signature.
pub const SIGNATURE_BYTES: usize = 64;

/// A device's secret signing key. Deliberately opaque and redacted: a derived `Debug` is how keys
/// end up in logs and `tracing` spans (the keystore task's rule, applied here too).
#[derive(Clone)]
pub struct DeviceSigningKey(SigningKey);

impl DeviceSigningKey {
    /// Wraps raw seed bytes from the keystore.
    pub fn from_bytes(bytes: [u8; SIGNING_KEY_BYTES]) -> DeviceSigningKey {
        DeviceSigningKey(SigningKey::from_bytes(&bytes))
    }

    /// The matching public key, safe to print and to ship to peers.
    pub fn public_key(&self) -> DevicePublicKey {
        DevicePublicKey(self.0.verifying_key())
    }
}

impl fmt::Debug for DeviceSigningKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("DeviceSigningKey(<redacted>)")
    }
}

/// A device's public signing key, as learned during pairing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DevicePublicKey(VerifyingKey);

impl DevicePublicKey {
    /// Parses 32 compressed bytes. A non-canonical or off-curve point is a typed error, not a panic.
    pub fn from_bytes(bytes: [u8; PUBLIC_KEY_BYTES]) -> Result<DevicePublicKey, CryptoError> {
        VerifyingKey::from_bytes(&bytes)
            .map(DevicePublicKey)
            .map_err(|_| CryptoError::BadPublicKey { device: None })
    }

    /// The compressed bytes, to store or ship.
    pub fn to_bytes(self) -> [u8; PUBLIC_KEY_BYTES] {
        self.0.to_bytes()
    }
}

/// An Ed25519 signature over one op's `signing_bytes`. Public (it travels on the wire and in the
/// `ops.signature` column) — only the *signing* key needs the redaction discipline.
///
/// `Serialize`/`Deserialize` are hand-written, not derived: `SIGNATURE_BYTES` is 64, and serde's
/// built-in array impls only cover `[T; 0..=32]`. Encoded as a byte string rather than a fixed
/// array (postcard then length-prefixes it) rather than pulling in `serde-big-array` for one type.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Signature([u8; SIGNATURE_BYTES]);

impl Serialize for Signature {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_bytes(&self.0)
    }
}

impl<'de> Deserialize<'de> for Signature {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Signature, D::Error> {
        struct SignatureVisitor;
        impl serde::de::Visitor<'_> for SignatureVisitor {
            type Value = Signature;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{SIGNATURE_BYTES} signature bytes")
            }
            fn visit_bytes<E: serde::de::Error>(self, v: &[u8]) -> Result<Signature, E> {
                let bytes: [u8; SIGNATURE_BYTES] = v
                    .try_into()
                    .map_err(|_| E::invalid_length(v.len(), &self))?;
                Ok(Signature(bytes))
            }
        }
        deserializer.deserialize_bytes(SignatureVisitor)
    }
}

impl Signature {
    /// Wraps raw signature bytes read from the store or the wire.
    pub fn from_bytes(bytes: [u8; SIGNATURE_BYTES]) -> Signature {
        Signature(bytes)
    }

    /// The raw bytes, for the `ops.signature` column and the wire.
    pub fn to_bytes(self) -> [u8; SIGNATURE_BYTES] {
        self.0
    }
}

impl fmt::Debug for Signature {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // A signature is public, but 64 raw bytes in a log line is noise; show a short prefix.
        write!(f, "Signature({:02x}{:02x}..)", self.0[0], self.0[1])
    }
}

/// Signs `op` with `key` over its canonical bytes.
pub fn sign(op: &Op, key: &DeviceSigningKey) -> Result<Signature, CryptoError> {
    let bytes = op.signing_bytes()?;
    let signature = key.0.sign(&bytes);
    Ok(Signature(signature.to_bytes()))
}

/// Verifies one op's signature under `key`. The origin is `op.hlc.device`, so a signature cannot be
/// re-attributed to another device by editing the op — doing that changes `signing_bytes`.
pub fn verify(op: &Op, signature: &Signature, key: &DevicePublicKey) -> Result<(), CryptoError> {
    let bytes = op.signing_bytes()?;
    let dalek = ed25519_dalek::Signature::from_bytes(&signature.0);
    key.0
        .verify_strict(&bytes, &dalek)
        .map_err(|_| CryptoError::SignatureInvalid {
            device: op.hlc.device,
        })
}

/// Verifies a whole batch before any of it is inserted. A batch with one bad signature is refused
/// whole and returns `Err`; there is no partial success, so the caller must not insert on `Err`.
///
/// `keys` maps an origin device to its public key. A missing key is `UnknownDevice`, never a skip.
/// Ops and signatures are parallel slices, exactly as the `ops` rows carry them.
pub fn verify_batch(
    ops: &[Op],
    signatures: &[Signature],
    keys: &BTreeMap<DeviceId, DevicePublicKey>,
) -> Result<(), CryptoError> {
    if ops.len() != signatures.len() {
        return Err(CryptoError::BatchLength {
            ops: ops.len(),
            signatures: signatures.len(),
        });
    }
    for (op, signature) in ops.iter().zip(signatures) {
        let device = op.hlc.device;
        let key = keys
            .get(&device)
            .ok_or(CryptoError::UnknownDevice { device })?;
        verify(op, signature, key)?;
    }
    debug_assert_eq!(ops.len(), signatures.len(), "checked above");
    Ok(())
}
