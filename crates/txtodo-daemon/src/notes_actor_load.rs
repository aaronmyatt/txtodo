//! Restoring a notes actor's Loro mirror at open (`NotesActor::open`), split out of
//! `notes_actor.rs` for its file budget. A child module, so it shares the actor's private items.

use super::{NotesActor, NotesActorConfig, mirror_err};
use crate::actor::SharedStore;
use crate::handle::ActorError;
use crate::history::MAX_REPLAY_PAGES;
use crate::notes_mirror::NotesMirror;
use crate::notes_mirror::loro_peer;
use crate::notes_state::NotesState;
use std::sync::PoisonError;
use txtodo_model::{FilePath, Op};
use txtodo_store::{MAX_OPS_PER_READ, Seq};

impl NotesActor {
    /// The mirror and the text to start from. A mirror that will not restore (an op since its
    /// snapshot that does not fit, say a skipped one after a crash before the snapshot moved) is
    /// rebuilt from the projection as a new lineage: the mirror never decides bytes, and a notes
    /// file that will not open refuses every peer op for it.
    pub(super) fn load(
        cfg: &NotesActorConfig,
        store: &SharedStore,
        projection: Option<Vec<u8>>,
        mirror_snapshot: Option<(Vec<u8>, Seq)>,
    ) -> Result<(NotesMirror, Vec<u8>), ActorError> {
        let peer = loro_peer(cfg.device);
        if let Some((snap, since)) = mirror_snapshot {
            match (
                Self::restore_mirror(store, &cfg.path, &snap, since, peer),
                projection,
            ) {
                (Ok(mirror), Some(bytes)) => return Ok((mirror, bytes)),
                (Ok(mirror), None) => {
                    let bytes = mirror.text().into_bytes();
                    return Ok((mirror, bytes));
                }
                (Err(e), None) => return Err(e),
                (Err(e), Some(bytes)) => {
                    tracing::warn!(file = %cfg.path, error = %e, "notes_mirror_restore_failed");
                    return Self::fresh(cfg, peer, bytes);
                }
            }
        }
        Self::fresh(cfg, peer, projection.unwrap_or_default())
    }

    fn fresh(
        cfg: &NotesActorConfig,
        peer: u64,
        bytes: Vec<u8>,
    ) -> Result<(NotesMirror, Vec<u8>), ActorError> {
        let state = NotesState::from_bytes(cfg.path.clone(), &bytes)?;
        let mirror = NotesMirror::from_state(&state, peer).map_err(mirror_err)?;
        Ok((mirror, bytes))
    }

    /// The persisted mirror plus the ops committed since it was taken (bounded paging, same shape
    /// as `history::replay`'s).
    fn restore_mirror(
        store: &SharedStore,
        path: &FilePath,
        snapshot: &[u8],
        since: Seq,
        peer: u64,
    ) -> Result<NotesMirror, ActorError> {
        let mut mirror = NotesMirror::from_snapshot(snapshot, path, peer).map_err(mirror_err)?;
        let mut since = since;
        for _page in 0..MAX_REPLAY_PAGES {
            let ops = {
                let guard = store.lock().unwrap_or_else(PoisonError::into_inner);
                guard.for_file(path, since)?
            };
            let Some(last) = ops.last() else { break };
            let plain: Vec<Op> = ops.iter().map(|s| s.op.clone()).collect();
            mirror.flush(&plain).map_err(mirror_err)?;
            since = last.seq;
            if ops.len() < MAX_OPS_PER_READ {
                break;
            }
        }
        Ok(mirror)
    }
}
