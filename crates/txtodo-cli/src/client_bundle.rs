//! `Daemon` bundle export/import RPCs (plan M8 `cli-bundle`), split out of `client.rs` purely for
//! its own file-length budget — same `Daemon` type, `rt`/`client`/`selector` fields `pub(crate)`
//! so this module (a sibling, not a submodule) can drive them directly.

use crate::client::{ClientError, Daemon};
use txtodo_proto::v1::{self as pb};

impl Daemon {
    /// Streams one bundle out of the daemon (plan M8 `cli-bundle`), handing each raw chunk to
    /// `on_chunk` as it arrives (`bundle.rs`'s own length-prefixed framing owns what it becomes
    /// on disk). `passphrase` is loopback-only KDF input for the daemon's own wrap; this crate
    /// never derives a key itself (it may not depend on txtodo-sync/txtodo-store).
    pub fn bundle_export(
        &mut self,
        passphrase: Vec<u8>,
        mut on_chunk: impl FnMut(&[u8]) -> std::io::Result<()>,
    ) -> Result<(), ClientError> {
        let req = pb::BundleExportRequest {
            passphrase,
            workspace: self.selector.clone(),
        };
        self.rt.block_on(async {
            let mut stream = self
                .client
                .bundle_export(req)
                .await
                .map_err(ClientError::Rpc)?
                .into_inner();
            while let Some(chunk) = stream.message().await.map_err(ClientError::Rpc)? {
                on_chunk(&chunk.data).map_err(ClientError::Io)?;
            }
            Ok(())
        })
    }

    /// Streams one bundle into the daemon: `next_frame` pulls one raw frame at a time (`Ok(None)`
    /// at EOF) while a concurrent task drives the RPC, so this never buffers the whole bundle.
    /// The passphrase rides in request metadata (a client-streaming RPC's request type is fixed
    /// to the streamed item, so there is no per-call field for it).
    pub fn bundle_import(
        &mut self,
        passphrase: Vec<u8>,
        next_frame: impl FnMut() -> std::io::Result<Option<Vec<u8>>> + Send + 'static,
    ) -> Result<pb::BundleImportResponse, ClientError> {
        let (tx, rx) = tokio::sync::mpsc::channel::<pb::BundleChunk>(4);
        let mut req = tonic::Request::new(tokio_stream::wrappers::ReceiverStream::new(rx));
        crate::bundle::insert_passphrase(&mut req, &passphrase);
        crate::bundle::insert_workspace(&mut req, self.selector.as_ref());
        self.rt.block_on(async {
            let producer = tokio::spawn(crate::bundle::drain_frames(next_frame, tx));
            let rep = self
                .client
                .bundle_import(req)
                .await
                .map_err(ClientError::Rpc)?;
            match producer.await {
                Ok(Ok(())) => {}
                Ok(Err(e)) => return Err(ClientError::Io(e)),
                Err(_) => {} // panicked; the RPC's own result already tells the story
            }
            Ok(rep.into_inner())
        })
    }
}
