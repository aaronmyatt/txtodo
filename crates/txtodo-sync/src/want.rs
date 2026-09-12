//! Head diffing: which of the peer's ops we lack. Pure — two head maps in, ranges out.
//!
//! A head is "per origin device, the highest `origin_seq` we hold", dense from 1, so the ops we
//! lack from device `d` are exactly `local[d] + 1 ..= remote[d]` whenever `remote[d] > local[d]`.
//! Devices only the local side knows are not wanted (the peer cannot supply them). The result is
//! bounded by `MAX_WANT_RANGES`; the rest is asked for on the next round.

use crate::message::{Heads, MAX_WANT_RANGES, OriginRange};

/// The runs `remote` holds and `local` does not, in device order, at most `MAX_WANT_RANGES`.
pub fn want(local: &Heads, remote: &Heads) -> Vec<OriginRange> {
    let mut ranges = Vec::with_capacity(remote.len().min(MAX_WANT_RANGES));
    // Bounded by remote.len(), itself capped at MAX_HEADS on the wire.
    for (device, &their_head) in remote {
        if ranges.len() == MAX_WANT_RANGES {
            break;
        }
        let our_head = local.get(device).copied().unwrap_or(0);
        if their_head > our_head {
            ranges.push(OriginRange {
                device: *device,
                first: our_head + 1,
                last: their_head,
            });
        }
    }
    debug_assert!(ranges.len() <= MAX_WANT_RANGES);
    debug_assert!(
        ranges.iter().all(|r| r.first <= r.last),
        "every wanted run is forwards"
    );
    debug_assert!(
        ranges.windows(2).all(|w| w[0].device < w[1].device),
        "one run per device, in device order"
    );
    ranges
}

/// Advances `heads` past a run just committed. Committing `first..=last` for a device whose head
/// is `first - 1` is the normal case; anything else is a gap and is refused so a hole can never be
/// papered over.
pub fn advance(heads: &mut Heads, committed: &OriginRange) -> Result<(), Gap> {
    let head = heads.get(&committed.device).copied().unwrap_or(0);
    if committed.first != head + 1 {
        return Err(Gap {
            device_head: head,
            range: *committed,
        });
    }
    debug_assert!(committed.last >= committed.first);
    heads.insert(committed.device, committed.last);
    debug_assert_eq!(heads.get(&committed.device), Some(&committed.last));
    Ok(())
}

/// A committed run that does not start right after the head we hold.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Gap {
    /// The head we hold for that device.
    pub device_head: u64,
    /// The run that would leave a hole (or repeat) before it.
    pub range: OriginRange,
}

impl core::fmt::Display for Gap {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "run {}..={} does not follow head {} for {:?}",
            self.range.first, self.range.last, self.device_head, self.range.device
        )
    }
}

impl std::error::Error for Gap {}
