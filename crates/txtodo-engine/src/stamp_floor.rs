//! ADR 0040: a local edit is stamped past the newest peer stamp its document holds, even when
//! the skew guard refused to merge that stamp into the clock. Lab clock-skew seed 777: b1's edit
//! carried b1's own +7 min stamp; a1 refused the merge (bound 5 min), took the edit, and stamped
//! its own append, typed after it, older; the stamp-ordered rebuild (ADR 0034) slotted the append
//! in front, where it did not fit, and it was lost on both devices.
//!
//! The device clock is left alone (the guard still keeps one bad clock from dragging every
//! document forward); only a local stamp in the document that holds the far stamp moves past it.
//! A peer more than [`FLOOR_LIMIT_MS`] ahead is still refused outright: that is a broken clock,
//! not skew, and following it would push this document's stamps into its future for good.

use txtodo_model::Hlc;

/// How far ahead of this device's wall a held peer stamp may pull a local stamp.
pub const FLOOR_LIMIT_MS: u64 = 24 * 60 * 60 * 1000;

/// Remembers `stamp`, a peer stamp the clock refused to merge, as the floor.
pub fn hold(floor: &mut Option<Hlc>, stamp: Hlc) {
    *floor = Some(floor.map_or(stamp, |f| f.max(stamp)));
}

/// `ticked` (this device's next stamp), moved just past `floor` when it is not already past it
/// and the floor is within [`FLOOR_LIMIT_MS`] of `now_ms`. Compared by wall and counter: a stamp
/// equal to the floor's but for the device id is not "past" it.
pub fn past_floor(ticked: Hlc, floor: Option<Hlc>, now_ms: u64) -> Hlc {
    match floor {
        Some(f) if !is_past(ticked, f) && within_limit(f, now_ms) => just_past(f, ticked),
        _ => ticked,
    }
}

/// `a` sorts after `b` by wall and counter alone.
fn is_past(a: Hlc, b: Hlc) -> bool {
    (a.wall_ms, a.counter) > (b.wall_ms, b.counter)
}

/// Whether `floor` is near enough to follow; a broken clock past the limit is logged and left.
fn within_limit(floor: Hlc, now_ms: u64) -> bool {
    let near = floor.wall_ms <= now_ms.saturating_add(FLOOR_LIMIT_MS);
    if !near {
        tracing::warn!(floor_ms = floor.wall_ms, now_ms, "stamp_floor_too_far");
    }
    near
}

/// The stamp right after `floor`, as `ticked`'s device.
fn just_past(floor: Hlc, ticked: Hlc) -> Hlc {
    let next = match floor.counter.checked_add(1) {
        Some(counter) => Hlc {
            wall_ms: floor.wall_ms,
            counter,
            device: ticked.device,
        },
        None => Hlc {
            wall_ms: floor.wall_ms + 1,
            counter: 0,
            device: ticked.device,
        },
    };
    debug_assert!(is_past(next, floor));
    debug_assert!(next > ticked, "the floor only ever moves a stamp forward");
    next
}

#[cfg(test)]
mod tests {
    use super::*;
    use txtodo_model::{DeviceId, Ulid};

    fn hlc(device: u128, wall_ms: u64, counter: u16) -> Hlc {
        Hlc {
            wall_ms,
            counter,
            device: DeviceId::new(Ulid::from_u128(device)),
        }
    }

    const MIN: u64 = 60_000;

    #[test]
    fn a_local_stamp_moves_past_a_held_far_stamp() {
        let peer = hlc(2, 10 * MIN, 3);
        let mut floor = None;
        hold(&mut floor, peer);
        let next = past_floor(hlc(1, 3 * MIN, 0), floor, 3 * MIN);
        assert_eq!((next.wall_ms, next.counter), (10 * MIN, 4));
        assert_eq!(
            next.device,
            hlc(1, 0, 0).device,
            "still this device's stamp"
        );
    }

    #[test]
    fn a_stamp_already_past_the_floor_is_kept() {
        let floor = Some(hlc(2, 2 * MIN, 0));
        let ticked = hlc(1, 3 * MIN, 0);
        assert_eq!(past_floor(ticked, floor, 3 * MIN), ticked);
        assert_eq!(past_floor(ticked, None, 3 * MIN), ticked);
    }

    #[test]
    fn a_floor_past_a_day_ahead_is_a_broken_clock_and_ignored() {
        let floor = Some(hlc(2, 2 * FLOOR_LIMIT_MS, 0));
        let ticked = hlc(1, MIN, 0);
        assert_eq!(past_floor(ticked, floor, MIN), ticked);
    }

    #[test]
    fn hold_keeps_the_newest() {
        let mut floor = None;
        hold(&mut floor, hlc(2, 9 * MIN, 0));
        hold(&mut floor, hlc(2, 7 * MIN, 0));
        assert_eq!(floor.map(|f| f.wall_ms), Some(9 * MIN));
    }
}
