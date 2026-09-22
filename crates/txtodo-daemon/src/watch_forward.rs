//! `forward_changes`: one actor's changes into the merged `Watch` stream, with the ref's fresh
//! rule-5 progress attached (plan M5, tasks/proto-tree-progress). Split out of `server.rs` purely
//! to keep that file within its line budget, same pattern as `progress.rs`/`notes.rs`.

use crate::convert::to_flag;
use crate::convert::to_summary;
use crate::handle::{ActorHandle, ConflictRow};
use crate::progress::watch_progress_of;
use crate::server::TxtodoService;
use tokio::sync::broadcast::error::RecvError;
use tokio::sync::mpsc;
use tonic::Status;
use txtodo_proto::v1 as pb;

/// The `Change.path` that says "the workspace's layout changed, refetch it" (task
/// layout-hot-reload-clients): not a document, so no hash, ops or progress come with it.
pub const LAYOUT_CHANGE_PATH: &str = "txtodo.toml";

/// Forwards every layout change (`SharedLayout::set`: the RPC, or the watcher's hot reload of
/// `txtodo.toml`) into the merged Watch stream as a `Change` for [`LAYOUT_CHANGE_PATH`], until
/// either side hangs up. A lagged receiver just sends one more notice: the client refetches the
/// layout either way.
pub(crate) async fn forward_layout_changes(
    mut sub: tokio::sync::broadcast::Receiver<()>,
    tx: mpsc::Sender<Result<pb::Change, Status>>,
) {
    loop {
        match sub.recv().await {
            Ok(()) | Err(RecvError::Lagged(_)) => {}
            Err(RecvError::Closed) => return,
        }
        let item = pb::Change {
            path: LAYOUT_CHANGE_PATH.to_owned(),
            ..pb::Change::default()
        };
        if tx.send(Ok(item)).await.is_err() {
            return;
        }
    }
}

/// Forwards one actor's changes into the merged Watch stream until either side hangs up.
pub(crate) async fn forward_changes(
    svc: TxtodoService,
    h: ActorHandle,
    mut sub: tokio::sync::broadcast::Receiver<crate::handle::Change>,
    tx: mpsc::Sender<Result<pb::Change, Status>>,
) {
    // Bounded by the subscriber's lifetime: `tx.send` fails once the client is gone.
    loop {
        let item = match sub.recv().await {
            Ok(c) => pb::Change {
                path: c.path.to_string(),
                hash: c.hash.to_vec(),
                ops: c.ops.iter().map(to_summary).collect(),
                // Line numbers are not known on the broadcast path; ListConflicts has them.
                review: c
                    .review
                    .iter()
                    .map(|row| {
                        to_flag(&ConflictRow {
                            row: row.clone(),
                            line_number: 0,
                        })
                    })
                    .collect(),
                progress: watch_progress_of(&svc, &h).await,
            },
            Err(RecvError::Lagged(_)) => match h.get().await {
                Ok(c) => pb::Change {
                    path: h.path().to_string(),
                    hash: c.hash.to_vec(),
                    ops: Vec::new(),
                    review: Vec::new(),
                    progress: watch_progress_of(&svc, &h).await,
                },
                Err(_) => return,
            },
            Err(RecvError::Closed) => return,
        };
        if tx.send(Ok(item)).await.is_err() {
            return;
        }
    }
}
