//! The wire seam around `Ops`: signs+seals what a peer is about to send, verifies+opens what one
//! just received. `Session` never touches key material (its own docs: transport- and
//! store-agnostic); this is where `sign`/`verify_batch` (authorship, durable) and `seal`/`open`
//! (confidentiality, in flight) actually run. `sync-crypto-envelope`'s notes named exactly this
//! gap as its own follow-up rather than folding it into that task's already-large diff, and
//! `sync-reject-tests`'s audit found it was still open: `Session` decided what was legal but no
//! caller ever called the crypto functions at all.
//!
//! The op-sending path is [`seal_ops`]; the op-receiving path is [`open_ops`]. A caller that gets
//! `Ok` from `open_ops` may hand the `Message` straight to [`crate::Session::on_ops`], which then
//! re-checks the same signatures (cheap, and it must never trust a caller that skipped this step).

use std::collections::BTreeMap;
use std::fmt;

use txtodo_model::{DeviceId, Op};
use txtodo_store::WorkspaceId;

use crate::aead::{GroupKey, GroupKeys, SealFor, open as aead_open, seal as aead_seal};
use crate::crypto_error::CryptoError;
use crate::frame::{Frame, PROTOCOL_VERSION};
use crate::message::{GroupId, Message, MessageError, OriginRange};
use crate::sign::{DevicePublicKey, DeviceSigningKey, sign, verify_batch};

/// Why a sealed `Ops` frame could not be built or opened. Wraps the crypto and message layers
/// underneath so a caller matches one type instead of two independent `Result`s.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SealedOpsError {
    /// A signature could not be produced/verified, or the AEAD seal/open failed.
    Crypto(CryptoError),
    /// The plaintext did not encode/decode as a `Message`, or failed a cap.
    Message(MessageError),
}

impl fmt::Display for SealedOpsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SealedOpsError::Crypto(e) => write!(f, "{e}"),
            SealedOpsError::Message(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for SealedOpsError {}

impl From<CryptoError> for SealedOpsError {
    fn from(e: CryptoError) -> SealedOpsError {
        SealedOpsError::Crypto(e)
    }
}

impl From<MessageError> for SealedOpsError {
    fn from(e: MessageError) -> SealedOpsError {
        SealedOpsError::Message(e)
    }
}

/// Which group, key epoch and workspace to seal for, bundled so `seal_ops` stays inside the
/// workspace's five-parameter cap (`clippy::too_many_arguments`).
pub struct SealContext<'a> {
    /// The sync group this batch belongs to; bound into the AEAD associated data.
    pub group: GroupId,
    /// Which of the group's retained key generations to seal under.
    pub epoch: u32,
    /// Which workspace this batch's ops belong to (ADR 0021); bound into the AEAD associated
    /// data so a mislabelled batch is refused, not silently routed to the wrong workspace.
    pub workspace: WorkspaceId,
    /// The key for `epoch`.
    pub key: &'a GroupKey,
}

/// The op-sending path. Signs every op with `signing_key` (authorship, stays true forever), wraps
/// them in a `Message::Ops`, then seals the encoded body under `ctx` (confidentiality, only for
/// this hop). Nothing downstream of the returned `Frame` ever sees a plaintext or unsigned op.
pub fn seal_ops(
    ops: Vec<Op>,
    ranges: Vec<OriginRange>,
    signing_key: &DeviceSigningKey,
    ctx: &SealContext<'_>,
) -> Result<Frame, SealedOpsError> {
    let signatures = ops
        .iter()
        .map(|op| sign(op, signing_key))
        .collect::<Result<Vec<_>, _>>()?;
    let frame = Message::Ops {
        ops,
        signatures,
        ranges,
    }
    .encode()?;
    let for_ = SealFor {
        group: ctx.group,
        epoch: ctx.epoch,
        workspace: ctx.workspace,
    };
    let sealed = aead_seal(PROTOCOL_VERSION, for_, ctx.key, &frame.body)?;
    Ok(Frame {
        version: PROTOCOL_VERSION,
        body: sealed,
    })
}

/// The op-receiving path. Opens `frame`'s sealed body with `group_keys` — a wrong/unretained
/// group key or mislabelled workspace is refused here, before a single byte of `Message` is
/// parsed — decodes it, then verifies every op's signature against `device_keys`. All-or-nothing,
/// like `verify_batch` itself: one bad signature or one unrecognised device and `Ok` is never
/// returned.
pub fn open_ops(
    frame: &Frame,
    group: GroupId,
    workspace: WorkspaceId,
    group_keys: &GroupKeys,
    device_keys: &BTreeMap<DeviceId, DevicePublicKey>,
) -> Result<Message, SealedOpsError> {
    let plaintext = aead_open(PROTOCOL_VERSION, group, workspace, group_keys, &frame.body)?;
    let msg = Message::decode(&Frame {
        version: PROTOCOL_VERSION,
        body: plaintext,
    })?;
    if let Message::Ops {
        ops, signatures, ..
    } = &msg
    {
        verify_batch(ops, signatures, device_keys)?;
    }
    Ok(msg)
}
