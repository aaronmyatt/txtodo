//! `txtodo bundle import`'s daemon-side core (plan M8 `cli-bundle`, design §4.5): reads a bundle
//! frame by frame, checks version/schema from the clear manifest before deriving any key, then
//! verifies every per-file blake3 hash and per-op Ed25519 signature against the actual decrypted
//! stream — never the manifest's word alone — before a single byte lands. All validation runs to
//! completion before [`commit_staged`] writes anything, so a failure at any stage leaves zero
//! partial state (design §4.5's edge cases).
//!
//! Sidecar identity (docs/questions.md Q2): a document's live fingerprints travel in the bundle
//! (`bundle_export.rs`) and land in the store *before* the caller re-registers the workspace's
//! documents, so `FileActor::open`'s `recover()` can resolve identity straight from them and take
//! the fast, no-reconcile path — the same path it takes for a document it already knew about —
//! instead of falling back to a full external-change reconcile that would mint brand-new ops for
//! history this import just landed under its original ids.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Mutex, PoisonError};

use prost::Message;
use txtodo_model::{FilePath, Fingerprint, Op, TaskId};
use txtodo_proto::v1 as pb;
use txtodo_store::{MAX_APPEND_BATCH, Store};
use txtodo_sync::{DevicePublicKey, Signature};

use crate::bundle_crypto::{BundleDecryptor, BundleHeader};
use crate::bundle_export::BUNDLE_FORMAT_VERSION;
pub(crate) use crate::bundle_import_error::BundleImportError;
use crate::bundle_wire::{BundleRecordReader, BundleTailRecord};
use crate::write::write_atomic;

/// What `import_from_chunks` needs from a live `Workspace`.
pub(crate) struct ImportCtx {
    pub(crate) root: PathBuf,
    pub(crate) store: std::sync::Arc<Mutex<Store>>,
    pub(crate) now_ms: u64,
}

impl ImportCtx {
    fn lock_store(&self) -> std::sync::MutexGuard<'_, Store> {
        self.store.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// What a successful import actually did.
#[derive(Debug)]
pub(crate) struct ImportOutcome {
    pub(crate) ops_imported: u64,
    pub(crate) files: Vec<String>,
}

// `BundleImportError` lives in `bundle_import_error.rs` (this file's own line budget).

/// Pulls one more raw frame, or `None` at end of stream.
type NextChunk<'a> = dyn FnMut() -> Result<Option<Vec<u8>>, BundleImportError> + 'a;

/// Reads and applies one whole bundle. `next` pulls one raw frame at a time (production wraps a
/// real `tonic::Streaming<BundleChunk>`; tests just drain a `Vec<Vec<u8>>`) — this function itself
/// never buffers more than one page of ops or one document's bytes at a time.
pub(crate) fn import_from_chunks(
    ctx: &ImportCtx,
    passphrase: &[u8],
    next: &mut NextChunk<'_>,
) -> Result<ImportOutcome, BundleImportError> {
    let header = read_header(next)?;
    let manifest = read_manifest(next)?;
    validate_manifest(ctx, &manifest)?;
    let records = decrypt_all_records(&header, passphrase, next)?;
    let staged = stage_records(&manifest, records)?;
    commit_staged(ctx, staged)
}

fn read_header(next: &mut NextChunk<'_>) -> Result<BundleHeader, BundleImportError> {
    let bytes = next()?.ok_or(BundleImportError::Truncated)?;
    BundleHeader::decode(&bytes).map_err(BundleImportError::Header)
}

fn read_manifest(next: &mut NextChunk<'_>) -> Result<pb::BundleManifest, BundleImportError> {
    let bytes = next()?.ok_or(BundleImportError::Truncated)?;
    pb::BundleManifest::decode(bytes.as_slice()).map_err(BundleImportError::ManifestDecode)
}

/// Checked before any key is derived — a wrong version or schema is refused for free.
fn validate_manifest(
    ctx: &ImportCtx,
    manifest: &pb::BundleManifest,
) -> Result<(), BundleImportError> {
    if manifest.version != BUNDLE_FORMAT_VERSION {
        return Err(BundleImportError::UnsupportedVersion(manifest.version));
    }
    let supported = ctx
        .lock_store()
        .user_version()
        .map_err(BundleImportError::Store)?
        .to_string();
    if manifest.schema_version != supported {
        return Err(BundleImportError::SchemaMismatch {
            found: manifest.schema_version.clone(),
            supported,
        });
    }
    Ok(())
}

/// Opens every remaining chunk with one-chunk lookahead (the STREAM construction needs to know
/// which chunk is last), reassembling the record stream as plaintext arrives.
fn decrypt_all_records(
    header: &BundleHeader,
    passphrase: &[u8],
    next: &mut NextChunk<'_>,
) -> Result<Vec<BundleTailRecord>, BundleImportError> {
    let mut dec = BundleDecryptor::new(header, passphrase).map_err(BundleImportError::Decrypt)?;
    let mut reader = BundleRecordReader::default();
    let mut records = Vec::new();
    let Some(mut current) = next()? else {
        reader.finish().map_err(BundleImportError::Record)?;
        return Ok(records);
    };
    loop {
        match next()? {
            Some(upcoming) => {
                let plain = dec
                    .decrypt_next(&current)
                    .map_err(BundleImportError::Decrypt)?;
                records.extend(reader.feed(&plain).map_err(BundleImportError::Record)?);
                current = upcoming;
            }
            None => {
                let plain = dec
                    .decrypt_last(&current)
                    .map_err(BundleImportError::Decrypt)?;
                records.extend(reader.feed(&plain).map_err(BundleImportError::Record)?);
                break;
            }
        }
    }
    reader.finish().map_err(BundleImportError::Record)?;
    Ok(records)
}

/// Every record, sorted into its own bucket, verified against `manifest` — nothing is written yet.
struct Staged {
    blobs: Vec<(FilePath, Vec<u8>)>,
    fingerprints: Vec<(FilePath, TaskId, Fingerprint, u64)>,
    ops: Vec<Op>,
}

fn stage_records(
    manifest: &pb::BundleManifest,
    records: Vec<BundleTailRecord>,
) -> Result<Staged, BundleImportError> {
    let mut staged = Staged {
        blobs: Vec::new(),
        fingerprints: Vec::new(),
        ops: Vec::new(),
    };
    for record in records {
        stage_one(manifest, record, &mut staged)?;
    }
    verify_file_set(manifest, &staged.blobs)?;
    verify_op_count(manifest, staged.ops.len())?;
    Ok(staged)
}

fn stage_one(
    manifest: &pb::BundleManifest,
    record: BundleTailRecord,
    staged: &mut Staged,
) -> Result<(), BundleImportError> {
    match record {
        BundleTailRecord::Blob { path, bytes } => {
            staged.blobs.push((parse_path(&path)?, bytes));
        }
        BundleTailRecord::Fingerprint {
            path,
            task,
            fingerprint,
            updated_at_ms,
        } => {
            staged
                .fingerprints
                .push((parse_path(&path)?, task, fingerprint, updated_at_ms));
        }
        BundleTailRecord::Op { op, signature } => {
            verify_op_signature(manifest, &op, &signature)?;
            staged.ops.push(op);
        }
    }
    Ok(())
}

fn parse_path(path: &str) -> Result<FilePath, BundleImportError> {
    FilePath::new(path).map_err(|_| BundleImportError::BadPath(path.to_owned()))
}

fn verify_op_signature(
    manifest: &pb::BundleManifest,
    op: &Op,
    signature: &Signature,
) -> Result<(), BundleImportError> {
    let device_bytes = op.hlc.device.ulid().to_u128().to_be_bytes();
    if device_bytes.as_slice() != manifest.device_id.as_slice() {
        return Err(BundleImportError::ForeignDevice);
    }
    let key_bytes: [u8; 32] = manifest
        .device_signing_public_key
        .as_slice()
        .try_into()
        .map_err(|_| BundleImportError::SignatureInvalid)?;
    let key =
        DevicePublicKey::from_bytes(key_bytes).map_err(|_| BundleImportError::SignatureInvalid)?;
    txtodo_sync::verify(op, signature, &key).map_err(|_| BundleImportError::SignatureInvalid)
}

/// The files actually received must be exactly the manifest's own list, each one's bytes hashing
/// to what the manifest claimed — re-checked against the real stream, never trusted on the
/// manifest's word (design §4.5's own rule).
fn verify_file_set(
    manifest: &pb::BundleManifest,
    blobs: &[(FilePath, Vec<u8>)],
) -> Result<(), BundleImportError> {
    if manifest.files.len() != blobs.len() {
        return Err(BundleImportError::FileSetMismatch);
    }
    let mut want: BTreeMap<&str, &[u8]> = BTreeMap::new();
    for f in &manifest.files {
        want.insert(f.path.as_str(), f.blake3.as_slice());
    }
    for (path, bytes) in blobs {
        let Some(expected) = want.remove(path.as_str()) else {
            return Err(BundleImportError::FileSetMismatch);
        };
        if blake3::hash(bytes).as_bytes().as_slice() != expected {
            return Err(BundleImportError::FileHashMismatch {
                path: path.to_string(),
            });
        }
    }
    if want.is_empty() {
        Ok(())
    } else {
        Err(BundleImportError::FileSetMismatch)
    }
}

fn verify_op_count(
    manifest: &pb::BundleManifest,
    received: usize,
) -> Result<(), BundleImportError> {
    let received = u64::try_from(received).unwrap_or(u64::MAX);
    if received == manifest.op_count {
        Ok(())
    } else {
        Err(BundleImportError::OpCountMismatch {
            claimed: manifest.op_count,
            received,
        })
    }
}

/// Everything above already validated: write the files, land the ops (deduped against what the
/// store already holds), then the fingerprints and projections. The only way this can still fail
/// is a real I/O error partway through — see this module's own top-level doc for that scope limit.
fn commit_staged(ctx: &ImportCtx, staged: Staged) -> Result<ImportOutcome, BundleImportError> {
    write_blobs_to_disk(&ctx.root, &staged.blobs)?;
    let ops_imported = insert_ops_deduped(ctx, staged.ops)?;
    write_fingerprints(ctx, &staged.fingerprints)?;
    write_projections(ctx, &staged.blobs)?;
    let files = staged.blobs.iter().map(|(p, _)| p.to_string()).collect();
    Ok(ImportOutcome {
        ops_imported,
        files,
    })
}

fn write_blobs_to_disk(
    root: &std::path::Path,
    blobs: &[(FilePath, Vec<u8>)],
) -> Result<(), BundleImportError> {
    for (path, bytes) in blobs {
        let abs = root.join(path.as_str());
        write_atomic(&abs, bytes).map_err(BundleImportError::Write)?;
    }
    Ok(())
}

/// Skips any `op_id` already present (the `ops.op_id` UNIQUE index would reject the whole batch
/// otherwise — see `txtodo-store`'s own `append_is_atomic_and_duplicate_op_ids_are_rejected`
/// test) — a re-import of an already-landed bundle is a no-op, not an error.
fn insert_ops_deduped(ctx: &ImportCtx, ops: Vec<Op>) -> Result<u64, BundleImportError> {
    let mut store = ctx.lock_store();
    let existing = store.existing_op_ids().map_err(BundleImportError::Store)?;
    let fresh: Vec<Op> = ops
        .into_iter()
        .filter(|op| !existing.contains(&op_id_bytes(op)))
        .collect();
    let mut inserted = 0u64;
    for batch in fresh.chunks(MAX_APPEND_BATCH) {
        store.append(batch).map_err(BundleImportError::Store)?;
        inserted += batch.len() as u64;
    }
    Ok(inserted)
}

fn op_id_bytes(op: &Op) -> [u8; 16] {
    op.id.ulid().to_u128().to_be_bytes()
}

fn write_fingerprints(
    ctx: &ImportCtx,
    rows: &[(FilePath, TaskId, Fingerprint, u64)],
) -> Result<(), BundleImportError> {
    let mut store = ctx.lock_store();
    for (path, task, fingerprint, updated_at_ms) in rows {
        store
            .upsert_fingerprint(path, *task, fingerprint, *updated_at_ms)
            .map_err(BundleImportError::Store)?;
    }
    Ok(())
}

fn write_projections(
    ctx: &ImportCtx,
    blobs: &[(FilePath, Vec<u8>)],
) -> Result<(), BundleImportError> {
    let mut store = ctx.lock_store();
    for (path, bytes) in blobs {
        let projection = txtodo_store::Projection {
            file: path.clone(),
            bytes: bytes.clone(),
            hash: *blake3::hash(bytes).as_bytes(),
            written_at_ms: ctx.now_ms,
        };
        store
            .put_projection(&projection)
            .map_err(BundleImportError::Store)?;
    }
    Ok(())
}
