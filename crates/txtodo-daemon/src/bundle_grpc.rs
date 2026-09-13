//! `BundleExport`/`BundleImport` (plan M8 `cli-bundle`, design §4.5): the async gRPC surface over
//! `bundle_export.rs`/`bundle_import.rs`'s synchronous core, the same split as `tokens.rs`/
//! `devices_grpc.rs`. Both RPCs run their (potentially slow, disk- and CPU-bound) work on a
//! blocking thread (`tokio::task::spawn_blocking`) rather than the async executor — the same
//! bridge `txtodo_sync::IrohLink` uses to drive a synchronous trait from async code, just in the
//! other direction here (a synchronous core driven from an async RPC).

use std::pin::Pin;
use std::sync::PoisonError;

use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Request, Response, Status, Streaming};
use txtodo_proto::v1 as pb;

use crate::bundle_export::{BundleExportError, ExportCtx, export_into};
use crate::bundle_import::{ImportCtx, ImportOutcome, import_from_chunks};
use crate::bundle_import_error::BundleImportError;
use crate::server::TxtodoService;

/// gRPC request metadata key carrying `BundleImport`'s passphrase — a client-streaming RPC's
/// request type is fixed to the streamed item (`BundleChunk`), so there is no per-call field to
/// put it in, unlike `BundleExportRequest.passphrase`. `-bin` is tonic's binary-metadata suffix:
/// the passphrase travels as raw bytes, not a header restricted to ASCII.
const PASSPHRASE_METADATA_KEY: &str = "x-txtodo-bundle-passphrase-bin";

/// Response stream type for `BundleExport`.
pub(crate) type BundleExportStream =
    Pin<Box<dyn tokio_stream::Stream<Item = Result<pb::BundleChunk, Status>> + Send>>;

/// Chunks buffered between the blocking exporter and the async response stream; small, since a
/// slow receiver simply back-pressures the exporter rather than piling chunks up in memory.
const EXPORT_CHANNEL_CAP: usize = 4;

fn status_of_export(e: BundleExportError) -> Status {
    Status::internal(e.to_string())
}

fn status_of_import(e: BundleImportError) -> Status {
    use BundleImportError as E;
    match e {
        E::Decrypt(_) => Status::unauthenticated(e.to_string()),
        E::UnsupportedVersion(_) | E::SchemaMismatch { .. } => {
            Status::failed_precondition(e.to_string())
        }
        E::FileSetMismatch
        | E::FileHashMismatch { .. }
        | E::OpCountMismatch { .. }
        | E::ForeignDevice
        | E::SignatureInvalid
        | E::BadPath(_)
        | E::Record(_)
        | E::ManifestDecode(_)
        | E::Header(_)
        | E::Truncated => Status::invalid_argument(e.to_string()),
        E::Write(_) | E::Store(_) => Status::internal(e.to_string()),
    }
}

impl TxtodoService {
    /// Streams one bundle for this workspace (`ExportCtx` gathers everything needed from the
    /// workspace up front, so the read lock is held only briefly, not for the whole export).
    pub(crate) async fn bundle_export_impl(
        &self,
        r: Request<pb::BundleExportRequest>,
    ) -> Result<Response<BundleExportStream>, Status> {
        let passphrase = r.into_inner().passphrase;
        let ctx = self.export_ctx()?;
        let (tx, rx) = mpsc::channel(EXPORT_CHANNEL_CAP);
        tokio::task::spawn_blocking(move || run_export(&ctx, &passphrase, &tx));
        Ok(Response::new(Box::pin(ReceiverStream::new(rx))))
    }

    /// Gathers what `export_into` needs from the live workspace; a short-lived read lock.
    fn export_ctx(&self) -> Result<ExportCtx, Status> {
        let ws = self.workspace();
        let signing = crate::keystore_setup::load_or_mint_device_signing(ws.key_store().as_ref())
            .map_err(|e| Status::internal(e.to_string()))?;
        Ok(ExportCtx {
            root: ws.root().to_path_buf(),
            store: ws.store().clone(),
            device: ws.device(),
            signing,
        })
    }

    /// Reads a bundle from the client stream and applies it, all-or-nothing.
    pub(crate) async fn bundle_import_impl(
        &self,
        r: Request<Streaming<pb::BundleChunk>>,
    ) -> Result<Response<pb::BundleImportResponse>, Status> {
        let passphrase = passphrase_from_metadata(&r)?;
        let ctx = self.import_ctx();
        let stream = r.into_inner();
        let handle = tokio::runtime::Handle::current();
        let outcome = tokio::task::spawn_blocking(move || {
            let mut stream = stream;
            let mut next = || pull_chunk(&handle, &mut stream);
            import_from_chunks(&ctx, &passphrase, &mut next)
        })
        .await
        .map_err(|e| Status::internal(format!("import task panicked: {e}")))?
        .map_err(status_of_import)?;
        self.rediscover_after_import()?;
        Ok(Response::new(response_of(outcome)))
    }

    fn import_ctx(&self) -> ImportCtx {
        let ws = self.workspace();
        ImportCtx {
            root: ws.root().to_path_buf(),
            store: ws.store().clone(),
            now_ms: crate::clock::Clock::now_ms(&crate::clock::SystemClock),
        }
    }

    /// Registers an actor for every document the import just wrote, so the running daemon serves
    /// them immediately rather than only after a restart.
    fn rediscover_after_import(&self) -> Result<(), Status> {
        let root = self.workspace().root().to_path_buf();
        let shared = self.shared_workspace();
        let mut ws = shared.write().unwrap_or_else(PoisonError::into_inner);
        ws.discover(&root)
            .map(|_| ())
            .map_err(|e| Status::internal(e.to_string()))
    }
}

fn response_of(outcome: ImportOutcome) -> pb::BundleImportResponse {
    pb::BundleImportResponse {
        ops_imported: outcome.ops_imported,
        files: outcome.files,
    }
}

fn passphrase_from_metadata<T>(r: &Request<T>) -> Result<Vec<u8>, Status> {
    let value = r
        .metadata()
        .get_bin(PASSPHRASE_METADATA_KEY)
        .ok_or_else(|| Status::invalid_argument("missing bundle passphrase metadata"))?;
    value
        .to_bytes()
        .map(|b| b.to_vec())
        .map_err(|_| Status::invalid_argument("bad bundle passphrase metadata"))
}

/// Pulls one chunk from a real `Streaming<BundleChunk>`, blocking this (already-blocking) thread
/// on the captured runtime handle — the same bridge `txtodo_sync::IrohLink::send`/`recv` use.
fn pull_chunk(
    handle: &tokio::runtime::Handle,
    stream: &mut Streaming<pb::BundleChunk>,
) -> Result<Option<Vec<u8>>, BundleImportError> {
    let msg = handle
        .block_on(stream.message())
        .map_err(|_| BundleImportError::Truncated)?;
    Ok(msg.map(|c| c.data))
}

/// Runs the synchronous exporter, forwarding each sealed chunk to `tx`; a closed receiver (the
/// client went away) simply stops the export early rather than erroring loudly.
fn run_export(
    ctx: &ExportCtx,
    passphrase: &[u8],
    tx: &mpsc::Sender<Result<pb::BundleChunk, Status>>,
) {
    let mut emit = |data: Vec<u8>| {
        tx.blocking_send(Ok(pb::BundleChunk { data }))
            .map_err(|_| BundleExportError::Emit("client disconnected".to_owned()))
    };
    if let Err(e) = export_into(ctx, passphrase, &mut emit) {
        let _ = tx.blocking_send(Err(status_of_export(e)));
    }
}
