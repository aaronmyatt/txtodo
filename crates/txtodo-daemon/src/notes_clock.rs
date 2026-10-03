//! The notes actor's clock follows the stamps it holds, as `FileActor`'s does (the HLC receive
//! rule in `sync_ops.rs`, the newest logged stamp in `external.rs`'s `recover`). Lab clock-skew
//! 1072683562: a1's notes clock never took b1's stamps, 2 minutes ahead, so a1's own append was
//! stamped older than b1's lines it was typed after. Text is rebuilt in stamp order (ADR 0034),
//! so the append was slotted in front of them, did not fit, and was lost on both devices.

use super::NotesActor;
use crate::handle::ActorError;
use std::sync::PoisonError;
use txtodo_model::{Hlc, Op};

impl NotesActor {
    /// At open: the clock moves past the newest op this file's log holds, never back.
    pub(super) fn adopt_logged_clock(&mut self) -> Result<(), ActorError> {
        let newest = {
            let guard = self.store.lock().unwrap_or_else(PoisonError::into_inner);
            guard.newest(&self.cfg.path, 1)?
        };
        if let Some(last) = newest.first() {
            let logged = Hlc {
                device: self.cfg.device,
                ..last.op.hlc
            };
            self.hlc = self.hlc.max(logged);
        }
        debug_assert_eq!(self.hlc.device, self.cfg.device);
        Ok(())
    }

    /// HLC receive rule for a peer batch's newest stamp, so this device's next edit sorts after
    /// every edit it holds. A stamp past the skew bound is not merged, the same refusal the link
    /// `Hello` makes.
    pub(super) fn observe_peer_stamps(&mut self, ops: &[Op]) {
        let Some(newest) = ops.iter().map(|op| op.hlc).max() else {
            return;
        };
        let before = self.hlc;
        if let Err(e) = self.hlc.merge(newest, self.clock.now_ms()) {
            log_stamp_not_merged(&self.cfg.path, &e);
        }
        debug_assert!(self.hlc >= before, "merge never goes back");
    }
}

/// <https://docs.rs/tracing/latest/tracing/macro.warn.html>
fn log_stamp_not_merged(path: &txtodo_model::FilePath, e: &txtodo_model::HlcError) {
    tracing::warn!(file = %path, error = %e, "sync_stamp_not_merged");
}
