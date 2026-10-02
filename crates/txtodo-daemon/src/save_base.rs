//! An editor save based on our previous write, not our latest (task partition-converge, chaos
//! 20261001-233439). An editor reads the file, a peer's add lands and we write it, and then the
//! editor renames its save over that write. Reconciled against our latest bytes, the save looks
//! like "the peer's line was replaced by the editor's"; under Sidecar identity, where lines carry
//! no `id:`, the reconciler matched them by position and sent an `EditText` over the peer's line.
//! The add was lost on every device.
//!
//! So each write keeps what the file held before it, for the own-write ring's TTL
//! (`expected.rs`, 2 s). A save that holds none of the lines our latest write added was written
//! from that earlier text: it is held as a pending save on that base, and `pending_save.rs`'s
//! three-way merge applies the editor's changes (base → disk) on the current state.
//!
//! The cost: a user who deletes every line our latest write added, within 2 s of that write, and
//! changes something else in the same save, gets those lines back. The own-write ring already
//! makes a bare undo to the previous bytes within 2 s a no-op, so this is the same window.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use crate::actor::FileActor;
use crate::expected::EXPECTED_WRITE_TTL_MS;
use crate::handle::ActorError;
use crate::pending_save::PendingSave;
use crate::state::DocState;
use txtodo_model::TaskId;

/// What the file held right before our latest write, and when that write happened.
pub(crate) struct PrevWrite {
    bytes: Vec<u8>,
    ids: Vec<Option<TaskId>>,
    at: Instant,
}

impl FileActor {
    /// After a write that replaced our own previous bytes: keep them as a possible save base.
    pub(crate) fn remember_prev_write(&mut self, bytes: Vec<u8>, before: &DocState) {
        self.prev_write = Some(PrevWrite {
            bytes,
            ids: before.line_ids().collect(),
            at: self.clock.now_instant(),
        });
    }

    /// `on_external_change` for `disk` that is not ours: true, with the save now pending on our
    /// previous write's bytes, when the save was written from them (see the module doc).
    pub(crate) fn hold_if_based_on_prev_write(&mut self, disk: &[u8]) -> Result<bool, ActorError> {
        let ttl = Duration::from_millis(EXPECTED_WRITE_TTL_MS);
        let Some(prev) = self.prev_write.take() else {
            return Ok(false);
        };
        let fresh = self.clock.now_instant().duration_since(prev.at) <= ttl;
        let added = lines_added(&prev.bytes, &self.projection);
        let based_on_prev = fresh
            && disk != prev.bytes.as_slice()
            && !added.is_empty()
            && added.iter().all(|line| !has_line(disk, line));
        if !based_on_prev {
            return Ok(false);
        }
        tracing::info!(file = %self.cfg.path, added = added.len(), "save_based_on_previous_write");
        self.hold(PendingSave::new(prev.bytes, prev.ids))?;
        Ok(true)
    }
}

/// The non-blank lines of `new` that `old` does not have (as many times as `new` has them more).
fn lines_added<'a>(old: &[u8], new: &'a [u8]) -> Vec<&'a [u8]> {
    let mut have: HashMap<&[u8], usize> = HashMap::new();
    for line in lines(old) {
        *have.entry(line).or_default() += 1;
    }
    let mut added = Vec::new();
    for line in lines(new) {
        match have.get_mut(line) {
            Some(n) if *n > 0 => *n -= 1,
            _ => added.push(line),
        }
    }
    debug_assert!(added.len() <= lines(new).count());
    added
}

fn has_line(text: &[u8], line: &[u8]) -> bool {
    lines(text).any(|l| l == line)
}

/// Lines without their `\n` or `\r\n`, blank ones left out.
fn lines(text: &[u8]) -> impl Iterator<Item = &[u8]> {
    text.split(|b| *b == b'\n')
        .map(|l| l.strip_suffix(b"\r").unwrap_or(l))
        .filter(|l| !l.iter().all(u8::is_ascii_whitespace))
}

#[cfg(test)]
mod tests {
    use super::lines_added;

    #[test]
    fn lines_added_counts_repeats_and_skips_blanks() {
        let added = lines_added(b"a\nb\n", b"a\nb\n\nc\na\r\n");
        assert_eq!(added, vec![&b"c"[..], &b"a"[..]]);
        assert!(
            lines_added(b"a\nb\n", b"b\na\n").is_empty(),
            "a reorder adds nothing"
        );
    }
}
