//! Batch confidentiality: XChaCha20-Poly1305 with the group key. The signature says *who* wrote an
//! op; this says *only the group may read it in flight*. The seal is stripped at import and the log
//! on disk is plaintext (the SQLite file is already as secret as the todo.txt beside it).
//!
//! XChaCha's 24-byte nonce is the whole point: random nonces are safe at our volumes, so there is no
//! counter to persist and no way to reuse one after a restore-from-backup. The nonce is drawn fresh
//! per batch from the OS CSPRNG, **never** from an injectable/seeded PRNG, so the deterministic
//! simulator (`tests/sim.rs`) cannot reach this path.
//!
//! The clear header binds the context: `version || group || epoch || workspace || nonce`. The same
//! `version || group || epoch || workspace` bytes are the AEAD associated data, so a foreign group,
//! a downgrade, or a mislabelled workspace fails the tag check rather than decrypting into (or
//! being routed as) something plausible. `epoch` names the group-key generation (`txtodo device
//! remove` rotates it); an unknown epoch is a typed error naming it, never a try-every-key loop.
//!
//! `workspace` exists because of ADR 0021 (task `daemon-shared-sync-link`): every workspace a
//! device opens now shares one group key (ADR 0021, `daemon-device-set-identity`, already landed),
//! so the group key alone can no longer tell two workspaces' sealed batches apart once their
//! traffic is multiplexed over one shared `Link`/connection/file-carrier directory. Binding it as
//! associated data (not just a bare unauthenticated prefix) matters for a real reason, not just
//! consistency: since the AEAD would happily decrypt *any* workspace's batch with the shared group
//! key, an unauthenticated tag could be swapped by a bug or an active relay without the tamper
//! being caught, silently routing one workspace's ops into another's oplog. Binding it into the
//! tag means a mislabelled batch fails to open at all, the same failure mode a foreign group
//! already gets.
//!
//! Refs: <https://docs.rs/chacha20poly1305> · XChaCha draft
//! <https://datatracker.ietf.org/doc/html/draft-irtf-cfrg-xchacha-03> · AEAD concept
//! <https://datatracker.ietf.org/doc/html/rfc5116>.

use std::collections::BTreeMap;
use std::fmt;

use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};

use crate::crypto_error::CryptoError;
use crate::message::GroupId;
use txtodo_store::WorkspaceId;

/// Bytes in a group key (XChaCha20-Poly1305).
pub const KEY_BYTES: usize = 32;
/// Bytes in an XChaCha nonce; large enough that random nonces never collide in practice.
pub const NONCE_BYTES: usize = 24;
/// Bytes Poly1305 appends to the ciphertext.
pub const TAG_BYTES: usize = 16;
/// Bytes of `version || group || epoch || workspace` bound as associated data.
pub const AAD_BYTES: usize = 2 + 16 + 4 + 16;
/// Clear sealed-batch header: `version (u16) || group (u128) || epoch (u32) || workspace (u128) ||
/// nonce (24)`.
pub const SEALED_HEADER_BYTES: usize = AAD_BYTES + NONCE_BYTES;
/// Group-key generations kept for reading history after a rotation. Old ops stay under their old key.
pub const MAX_RETAINED_KEY_EPOCHS: usize = 16;

/// One group key for one epoch. Opaque and redacted: never let a `Debug` print key material.
#[derive(Clone, PartialEq, Eq)]
pub struct GroupKey([u8; KEY_BYTES]);

impl GroupKey {
    /// Wraps raw key bytes from the keystore.
    pub const fn from_bytes(bytes: [u8; KEY_BYTES]) -> GroupKey {
        GroupKey(bytes)
    }

    /// Raw bytes, for `lan_op_signing`'s HKDF derivation. `pub(crate)`, not `pub`: nothing outside
    /// this crate ever sees a group key's bytes directly.
    pub(crate) fn as_bytes(&self) -> &[u8; KEY_BYTES] {
        &self.0
    }

    fn cipher(&self) -> XChaCha20Poly1305 {
        XChaCha20Poly1305::new((&self.0).into())
    }
}

impl fmt::Debug for GroupKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("GroupKey(<redacted>)")
    }
}

/// The group keys this device retains, one per epoch. A `BTreeMap` so iteration order is stable and
/// nothing hash-ordered ever touches key material.
#[derive(Clone, Debug, Default)]
pub struct GroupKeys {
    keys: BTreeMap<u32, GroupKey>,
}

impl GroupKeys {
    /// An empty set; the caller inserts the current epoch after pairing.
    pub fn new() -> GroupKeys {
        GroupKeys {
            keys: BTreeMap::new(),
        }
    }

    /// Adds a key for `epoch`, refusing to retain more than `MAX_RETAINED_KEY_EPOCHS`. Replacing an
    /// existing epoch never grows the set, so a rotation may overwrite.
    pub fn insert(&mut self, epoch: u32, key: GroupKey) -> Result<(), CryptoError> {
        if !self.keys.contains_key(&epoch) && self.keys.len() >= MAX_RETAINED_KEY_EPOCHS {
            return Err(CryptoError::TooManyEpochs {
                len: self.keys.len() + 1,
                max: MAX_RETAINED_KEY_EPOCHS,
            });
        }
        self.keys.insert(epoch, key);
        debug_assert!(self.keys.len() <= MAX_RETAINED_KEY_EPOCHS);
        Ok(())
    }

    /// The key for `epoch`, if retained.
    pub fn get(&self, epoch: u32) -> Option<&GroupKey> {
        self.keys.get(&epoch)
    }

    /// How many epochs are retained (reported in `UnknownEpoch`).
    pub fn len(&self) -> usize {
        self.keys.len()
    }

    /// True when no epoch is retained.
    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }
}

/// Which group, key epoch and workspace a batch is sealed for — bundled so [`seal`] stays inside
/// the workspace's five-parameter cap (the same reason `sealed_ops::SealContext` exists one layer
/// up, which now carries a `workspace` field of its own and builds one of these to call `seal`).
#[derive(Clone, Copy)]
pub struct SealFor {
    /// The sync group this batch belongs to; bound into the AEAD associated data.
    pub group: GroupId,
    /// Which of the group's retained key generations to seal under.
    pub epoch: u32,
    /// Which workspace this batch's ops belong to; bound into the AEAD associated data (see the
    /// module doc for why this must be authenticated, not a bare prefix).
    pub workspace: WorkspaceId,
}

/// `version || group || epoch || workspace`, little-endian, exactly as it appears in the clear
/// header.
fn aad(version: u16, for_: SealFor) -> [u8; AAD_BYTES] {
    let mut aad = [0u8; AAD_BYTES];
    aad[0..2].copy_from_slice(&version.to_le_bytes());
    aad[2..18].copy_from_slice(&for_.group.0.to_le_bytes());
    aad[18..22].copy_from_slice(&for_.epoch.to_le_bytes());
    aad[22..38].copy_from_slice(&for_.workspace.ulid().to_u128().to_le_bytes());
    aad
}

/// Seals `plaintext` for `for_.group`/`for_.workspace` under `for_.epoch`'s key, prefixing the
/// clear header. A fresh OS nonce per call; two seals of the same bytes never share a ciphertext.
/// Thin span wrapper around `seal_inner` (`#[instrument]` on the real body overflows
/// `cognitive_complexity`, `tasks/logging-sync-crate/notes.md`).
#[tracing::instrument(skip_all, fields(group = ?for_.group, workspace = %for_.workspace, epoch = for_.epoch))]
pub fn seal(
    version: u16,
    for_: SealFor,
    key: &GroupKey,
    plaintext: &[u8],
) -> Result<Vec<u8>, CryptoError> {
    let r = seal_inner(version, for_, key, plaintext);
    log_seal(&r);
    r
}

fn seal_inner(
    version: u16,
    for_: SealFor,
    key: &GroupKey,
    plaintext: &[u8],
) -> Result<Vec<u8>, CryptoError> {
    let mut nonce = [0u8; NONCE_BYTES];
    // Direct OS call, not a passed-in RNG: there is no seam for a seeded PRNG to slip through.
    getrandom::fill(&mut nonce).map_err(|_| CryptoError::Entropy)?;
    let aad = aad(version, for_);
    let ciphertext = key
        .cipher()
        .encrypt(
            XNonce::from_slice(&nonce),
            Payload {
                msg: plaintext,
                aad: &aad,
            },
        )
        .map_err(|_| CryptoError::Encrypt)?;
    let mut out = Vec::with_capacity(SEALED_HEADER_BYTES + ciphertext.len());
    out.extend_from_slice(&aad);
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&ciphertext);
    debug_assert_eq!(out.len(), SEALED_HEADER_BYTES + plaintext.len() + TAG_BYTES);
    Ok(out)
}

/// Opens a sealed batch whose header must match `version`, `group` and `workspace`, using `keys`
/// to find the epoch's key. Every mismatch is its own typed error; the ciphertext is never
/// touched on a header failure. Wrapper/inner split, same reason as `seal`.
#[tracing::instrument(skip_all, fields(group = ?group, workspace = %workspace))]
pub fn open(
    version: u16,
    group: GroupId,
    workspace: WorkspaceId,
    keys: &GroupKeys,
    sealed: &[u8],
) -> Result<Vec<u8>, CryptoError> {
    let r = open_inner(version, group, workspace, keys, sealed);
    log_open(&r);
    r
}

fn open_inner(
    version: u16,
    group: GroupId,
    workspace: WorkspaceId,
    keys: &GroupKeys,
    sealed: &[u8],
) -> Result<Vec<u8>, CryptoError> {
    let minimum = SEALED_HEADER_BYTES + TAG_BYTES;
    if sealed.len() < minimum {
        return Err(CryptoError::Truncated {
            needed: minimum,
            got: sealed.len(),
        });
    }
    let got_version = u16::from_le_bytes([sealed[0], sealed[1]]);
    if got_version != version {
        return Err(CryptoError::WrongVersion {
            got: got_version,
            expected: version,
        });
    }
    let got_group = read_u128(&sealed[2..18]);
    if got_group != group.0 {
        return Err(CryptoError::WrongGroup {
            got: GroupId(got_group),
            expected: group,
        });
    }
    let epoch = u32::from_le_bytes([sealed[18], sealed[19], sealed[20], sealed[21]]);
    let got_workspace = WorkspaceId::new(txtodo_model::Ulid::from_u128(read_u128(&sealed[22..38])));
    if got_workspace != workspace {
        return Err(CryptoError::WrongWorkspace {
            got: got_workspace,
            expected: workspace,
        });
    }
    let key = keys.get(epoch).ok_or(CryptoError::UnknownEpoch {
        epoch,
        held: keys.len(),
    })?;
    let aad = sealed[..AAD_BYTES].to_vec();
    let plaintext = key
        .cipher()
        .decrypt(
            XNonce::from_slice(&sealed[AAD_BYTES..SEALED_HEADER_BYTES]),
            Payload {
                msg: &sealed[SEALED_HEADER_BYTES..],
                aad: &aad,
            },
        )
        .map_err(|_| CryptoError::Decrypt { epoch })?;
    debug_assert_eq!(
        plaintext.len() + TAG_BYTES,
        sealed.len() - SEALED_HEADER_BYTES
    );
    Ok(plaintext)
}

/// Reads a sealed batch's workspace id straight out of its clear header, without a key and without
/// checking the AEAD tag — task `daemon-shared-sync-link`: an accept loop sharing one endpoint
/// across every open workspace needs to know *which* workspace a fresh connection's first frame is
/// probably for before it can even look up that workspace's group keys to call [`open`] for real.
/// **This is routing, not authentication.** A peeked id is not proven correct the way [`open`]'s
/// own `WrongWorkspace` check proves one: a caller must still route to the matching workspace and
/// call [`open`] there, which re-validates the same bytes against the AEAD tag before a single
/// byte of plaintext is trusted. `None` on anything shorter than the header — never a panic on a
/// truncated or malicious first read.
pub fn peek_workspace(sealed: &[u8]) -> Option<WorkspaceId> {
    let bytes = sealed.get(22..38)?;
    Some(WorkspaceId::new(txtodo_model::Ulid::from_u128(read_u128(
        bytes,
    ))))
}

/// Split out so the event macros don't count against `seal`'s own `#[instrument]` budget.
/// **Byte counts only, never the plaintext or ciphertext.** `warn!` on failure, not `debug!`: a
/// caller that cannot seal a batch at all is worth a human's attention immediately.
fn log_seal(r: &Result<Vec<u8>, CryptoError>) {
    match r {
        Ok(out) => log_seal_ok(out.len()),
        Err(e) => log_seal_failed(e.kind()),
    }
}

fn log_seal_ok(bytes: usize) {
    tracing::debug!(bytes, "seal_ok");
}

fn log_seal_failed(kind: &'static str) {
    tracing::warn!(kind, "seal_failed");
}

/// Split out so the event macros don't count against `open`'s own `#[instrument]` budget. `debug!`
/// on failure, not `warn!` (task sync-drift line 5): a device paired once and now in another group
/// fails every frame of every redial, 469 warnings a day on one Mac. Only the caller knows which
/// peer sent the frame, so the warning is its job: `txtodo-daemon`'s `peer_keys.rs` warns once per
/// peer and kind, and its file carrier logs `file_carrier_open_failed`.
fn log_open(r: &Result<Vec<u8>, CryptoError>) {
    match r {
        Ok(out) => log_open_ok(out.len()),
        Err(e) => log_open_failed(e.kind()),
    }
}

fn log_open_ok(bytes: usize) {
    tracing::debug!(bytes, "open_ok");
}

fn log_open_failed(kind: &'static str) {
    tracing::debug!(kind, "open_failed");
}

/// Reads 16 little-endian bytes without `try_into`, so a length check earlier in `open` is the only
/// precondition and there is no panic path here.
fn read_u128(bytes: &[u8]) -> u128 {
    let mut buf = [0u8; 16];
    buf.copy_from_slice(bytes);
    u128::from_le_bytes(buf)
}
