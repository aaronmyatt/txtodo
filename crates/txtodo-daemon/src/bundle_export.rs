//! `txtodo bundle export`'s daemon-side core (plan M8 `cli-bundle`, design §4.5): builds the
//! manifest, then streams every document's exact bytes (the "snapshot") and the whole op log (the
//! "tail") as a bounded, encrypted sequence of [`crate::bundle_wire::BundleTailRecord`]s. Split
//! from `bundle_grpc.rs` (the async gRPC surface) so this half is directly testable without a
//! socket — every function here is synchronous.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};

use prost::Message;
use txtodo_model::{DeviceId, FilePath};
use txtodo_proto::v1 as pb;
use txtodo_store::{Seq, Store, Stored};
use txtodo_sync::DeviceSigningKey;

use crate::bundle_crypto::{BundleCryptoError, BundleEncryptor, BundleHeader, CHUNK_PLAINTEXT_MAX};
use crate::bundle_wire::BundleTailRecord;
use crate::walker::{self, WalkError};

/// Bundle wire/format version (`BundleManifest.version`) — bumped whenever the encrypted body's
/// own record shapes change in a way an older importer could not read.
pub(crate) const BUNDLE_FORMAT_VERSION: u32 = 1;

/// Op-log rows read per `ops_page` call — bounded like every other read in this crate, so export
/// never holds the whole tail in memory at once.
const OPS_PAGE_SIZE: usize = 1_000;

/// What `export_into` needs from a live `Workspace`, gathered up front so the caller can drop its
/// lock on the workspace before the (possibly slow) streaming work runs.
pub(crate) struct ExportCtx {
    pub(crate) root: PathBuf,
    pub(crate) store: std::sync::Arc<Mutex<Store>>,
    pub(crate) device: DeviceId,
    pub(crate) signing: DeviceSigningKey,
}

impl ExportCtx {
    fn lock_store(&self) -> std::sync::MutexGuard<'_, Store> {
        self.store.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// Why an export failed.
#[derive(Debug)]
pub(crate) enum BundleExportError {
    /// A file could not be read.
    Io(PathBuf, std::io::Error),
    /// The store failed.
    Store(txtodo_store::StoreError),
    /// Discovery failed.
    Walk(WalkError),
    /// The header/AEAD layer failed.
    Crypto(BundleCryptoError),
    /// Signing an op failed.
    Sign(txtodo_sync::CryptoError),
    /// A record failed to encode.
    Encode(postcard::Error),
    /// The caller's `emit` sink refused a chunk (e.g. the gRPC client went away).
    Emit(String),
}

impl std::fmt::Display for BundleExportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BundleExportError::Io(p, e) => write!(f, "read {}: {e}", p.display()),
            BundleExportError::Store(e) => write!(f, "store: {e}"),
            BundleExportError::Walk(e) => write!(f, "discover documents: {e}"),
            BundleExportError::Crypto(e) => write!(f, "{e}"),
            BundleExportError::Sign(e) => write!(f, "sign op: {e}"),
            BundleExportError::Encode(e) => write!(f, "encode record: {e}"),
            BundleExportError::Emit(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for BundleExportError {}

fn read_file(root: &Path, path: &FilePath) -> Result<Vec<u8>, BundleExportError> {
    let abs = root.join(path.as_str());
    std::fs::read(&abs).map_err(|e| BundleExportError::Io(abs, e))
}

/// Seals a growing plaintext buffer into bounded [`CHUNK_PLAINTEXT_MAX`] STREAM chunks, handing
/// each sealed chunk to `emit` as soon as it is ready — the whole tail is never buffered at once.
struct ChunkWriter<'a> {
    enc: BundleEncryptor,
    buf: Vec<u8>,
    emit: &'a mut dyn FnMut(Vec<u8>) -> Result<(), BundleExportError>,
}

impl ChunkWriter<'_> {
    fn write_record(&mut self, record: &BundleTailRecord) -> Result<(), BundleExportError> {
        record
            .encode_into(&mut self.buf)
            .map_err(BundleExportError::Encode)?;
        while self.buf.len() >= CHUNK_PLAINTEXT_MAX {
            self.flush_full_chunk()?;
        }
        Ok(())
    }

    fn flush_full_chunk(&mut self) -> Result<(), BundleExportError> {
        let chunk: Vec<u8> = self.buf.drain(..CHUNK_PLAINTEXT_MAX).collect();
        let sealed = self
            .enc
            .encrypt_next(&chunk)
            .map_err(BundleExportError::Crypto)?;
        (self.emit)(sealed)
    }

    fn finish(self) -> Result<(), BundleExportError> {
        let ChunkWriter { enc, buf, emit } = self;
        let sealed = enc.encrypt_last(&buf).map_err(BundleExportError::Crypto)?;
        emit(sealed)
    }
}

/// Streams one whole bundle: the clear header, the clear manifest, then the encrypted body (every
/// document's bytes, then the whole op tail). `emit` receives each frame's raw bytes in order —
/// the caller decides what a frame becomes on the wire (one `BundleChunk`, one line in a test
/// fixture, ...). Bounded memory throughout: at most one [`CHUNK_PLAINTEXT_MAX`]-sized plaintext
/// buffer and one page ([`OPS_PAGE_SIZE`]) of ops in flight at any time.
pub(crate) fn export_into(
    ctx: &ExportCtx,
    passphrase: &[u8],
    emit: &mut dyn FnMut(Vec<u8>) -> Result<(), BundleExportError>,
) -> Result<(), BundleExportError> {
    let header = BundleHeader::fresh().map_err(BundleExportError::Crypto)?;
    emit(header.encode().to_vec())?;
    let manifest = build_manifest(ctx)?;
    emit(manifest.encode_to_vec())?;
    let enc = BundleEncryptor::new(&header, passphrase).map_err(BundleExportError::Crypto)?;
    let mut writer = ChunkWriter {
        enc,
        buf: Vec::new(),
        emit,
    };
    write_blobs(ctx, &mut writer)?;
    write_ops(ctx, &mut writer)?;
    writer.finish()
}

/// Every `todo.txt`/`notes.md` under the root, its exact bytes hashed for [`pb::FileHash`].
fn build_manifest(ctx: &ExportCtx) -> Result<pb::BundleManifest, BundleExportError> {
    let paths = walker::walk(&ctx.root).map_err(BundleExportError::Walk)?;
    let mut files = Vec::with_capacity(paths.len());
    for path in &paths {
        let bytes = read_file(&ctx.root, path)?;
        files.push(pb::FileHash {
            path: path.to_string(),
            blake3: blake3::hash(&bytes).as_bytes().to_vec(),
        });
    }
    let store = ctx.lock_store();
    let schema_version = store
        .user_version()
        .map_err(BundleExportError::Store)?
        .to_string();
    let op_count = store.total_ops().map_err(BundleExportError::Store)?;
    drop(store);
    Ok(pb::BundleManifest {
        version: BUNDLE_FORMAT_VERSION,
        schema_version,
        device_id: ctx.device.ulid().to_u128().to_be_bytes().to_vec(),
        device_signing_public_key: ctx.signing.public_key().to_bytes().to_vec(),
        files,
        op_count,
    })
}

/// Every document's exact bytes, then (for a task document) its live sidecar fingerprints — those
/// travel too so a sidecar-mode importer's `recover()` takes the fast, no-reconcile path instead
/// of minting brand-new ops for a document it just received (`bundle_import.rs`'s module doc).
fn write_blobs(ctx: &ExportCtx, writer: &mut ChunkWriter) -> Result<(), BundleExportError> {
    for path in walker::walk(&ctx.root).map_err(BundleExportError::Walk)? {
        let bytes = read_file(&ctx.root, &path)?;
        writer.write_record(&BundleTailRecord::Blob {
            path: path.to_string(),
            bytes,
        })?;
        write_fingerprints_for(ctx, &path, writer)?;
    }
    Ok(())
}

fn write_fingerprints_for(
    ctx: &ExportCtx,
    path: &FilePath,
    writer: &mut ChunkWriter,
) -> Result<(), BundleExportError> {
    let rows = ctx
        .lock_store()
        .live_fingerprints(path)
        .map_err(BundleExportError::Store)?;
    for row in rows {
        writer.write_record(&BundleTailRecord::Fingerprint {
            path: path.to_string(),
            task: row.task,
            fingerprint: row.fingerprint,
            updated_at_ms: row.updated_at_ms,
        })?;
    }
    Ok(())
}

/// The whole op log, every file, in `seq` order, one bounded page at a time.
fn write_ops(ctx: &ExportCtx, writer: &mut ChunkWriter) -> Result<(), BundleExportError> {
    let mut since = Seq(0);
    loop {
        let page = ctx
            .lock_store()
            .ops_page(since, OPS_PAGE_SIZE)
            .map_err(BundleExportError::Store)?;
        let Some(last) = page.last() else { break };
        since = last.seq;
        let full_page = page.len() == OPS_PAGE_SIZE;
        for stored in &page {
            write_one_op(ctx, stored, writer)?;
        }
        if !full_page {
            break;
        }
    }
    Ok(())
}

fn write_one_op(
    ctx: &ExportCtx,
    stored: &Stored,
    writer: &mut ChunkWriter,
) -> Result<(), BundleExportError> {
    let signature = txtodo_sync::sign(&stored.op, &ctx.signing).map_err(BundleExportError::Sign)?;
    writer.write_record(&BundleTailRecord::Op {
        op: stored.op.clone(),
        signature,
    })
}
