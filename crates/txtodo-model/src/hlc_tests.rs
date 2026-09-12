//! Hlc: send rule (`tick`), receive rule (`merge`), the asymmetric skew guard, and `Skew::check`.
//! `now_ms` is a parameter everywhere, so every case is a table row and nothing sleeps.

use crate::DeviceId;
use crate::hlc::{Hlc, HlcError, MAX_PEER_SKEW_AHEAD_MS, MAX_PEER_SKEW_BEHIND_MS, Skew};
use proptest::prelude::*;
use txtodo_core::Ulid;

const MINUTE_MS: u64 = 60 * 1_000;
const DAY_MS: u64 = 24 * 60 * MINUTE_MS;

fn dev(n: u128) -> DeviceId {
    DeviceId::new(Ulid::from_u128(n))
}

fn hlc(wall_ms: u64, counter: u16, device: u128) -> Hlc {
    Hlc {
        wall_ms,
        counter,
        device: dev(device),
    }
}

#[test]
fn tick_follows_the_wall_forward_and_counts_when_it_stalls_or_regresses() {
    let mut clock = Hlc::zero(dev(1));
    assert_eq!(clock.tick(1_000).unwrap(), hlc(1_000, 0, 1));
    assert_eq!(clock.tick(1_000).unwrap().counter, 1);
    assert_eq!(clock.tick(900).unwrap(), hlc(1_000, 2, 1));
    assert_eq!(clock.tick(1_001).unwrap(), hlc(1_001, 0, 1));
}

#[test]
fn counter_overflow_is_an_error_not_a_wrap() {
    let mut clock = hlc(5, u16::MAX, 1);
    assert_eq!(clock.tick(5), Err(HlcError::Overflow { wall_ms: 5 }));
    assert_eq!(
        clock.counter,
        u16::MAX,
        "a failed tick leaves the clock unchanged"
    );
    let mut full = hlc(5, u16::MAX, 1);
    assert_eq!(
        full.merge(hlc(5, 3, 2), 5),
        Err(HlcError::Overflow { wall_ms: 5 })
    );
    assert_eq!(
        full,
        hlc(5, u16::MAX, 1),
        "a failed merge leaves the clock unchanged"
    );
}

/// Kulkarni §3, the four branches of the receive rule: which wall wins decides the counter.
#[test]
fn merge_takes_the_larger_wall_and_the_counter_that_belongs_to_it() {
    // (local wall wins, remote wall wins) → expected counter.
    let rows: [(Hlc, Hlc, u64, Hlc); 4] = [
        // both equal the max: counter = max(counters) + 1
        (hlc(1_000, 4, 1), hlc(1_000, 9, 2), 900, hlc(1_000, 10, 1)),
        // only ours is the max: our counter + 1
        (hlc(1_000, 4, 1), hlc(500, 9, 2), 900, hlc(1_000, 5, 1)),
        // only the remote is the max: its counter + 1
        (hlc(500, 4, 1), hlc(1_000, 9, 2), 900, hlc(1_000, 10, 1)),
        // the wall clock moved us past both: counter resets
        (hlc(500, 4, 1), hlc(600, 9, 2), 1_000, hlc(1_000, 0, 1)),
    ];
    for (local, remote, now_ms, expected) in rows {
        let mut clock = local;
        let got = clock.merge(remote, now_ms).unwrap();
        assert_eq!(
            got, expected,
            "local {local:?} remote {remote:?} now {now_ms}"
        );
        assert_eq!(clock, expected, "merge stores what it returns");
        assert!(got > local && got > remote);
    }
}

#[test]
fn a_peer_just_past_five_minutes_ahead_is_refused_and_just_under_is_merged() {
    let now_ms = 10 * DAY_MS;
    let mut clock = hlc(now_ms, 0, 1);
    let too_far = now_ms + MAX_PEER_SKEW_AHEAD_MS + 1;
    assert_eq!(
        clock.merge(hlc(too_far, 0, 2), now_ms),
        Err(HlcError::PeerAhead {
            peer_ms: too_far,
            local_ms: now_ms,
            bound_ms: MAX_PEER_SKEW_AHEAD_MS,
        })
    );
    assert_eq!(
        clock,
        hlc(now_ms, 0, 1),
        "a refused peer leaves the clock unchanged"
    );
    let just_under = now_ms + 4 * MINUTE_MS + 59 * 1_000;
    assert_eq!(
        clock.merge(hlc(just_under, 0, 2), now_ms).unwrap(),
        hlc(just_under, 1, 1)
    );
    let exactly = now_ms + MAX_PEER_SKEW_AHEAD_MS;
    assert!(
        clock.merge(hlc(exactly, 0, 2), now_ms).is_ok(),
        "the bound is inclusive"
    );
}

#[test]
fn a_peer_a_day_behind_merges_and_skew_says_behind() {
    let now_ms = 10 * DAY_MS;
    let mut clock = hlc(now_ms, 2, 1);
    let old = hlc(now_ms - DAY_MS, 7, 2);
    assert_eq!(Skew::check(old.wall_ms, now_ms), Skew::Behind(DAY_MS));
    assert_eq!(
        clock.merge(old, now_ms).unwrap(),
        hlc(now_ms, 3, 1),
        "nothing of ours moves"
    );
    assert_eq!(clock.merge(old, now_ms).unwrap(), hlc(now_ms, 4, 1));
}

#[test]
fn skew_check_is_ok_within_both_bounds_and_names_the_distance_past_them() {
    let local = 10 * DAY_MS;
    assert_eq!(Skew::check(local, local), Skew::Ok);
    assert_eq!(Skew::check(local + MAX_PEER_SKEW_AHEAD_MS, local), Skew::Ok);
    assert_eq!(
        Skew::check(local - MAX_PEER_SKEW_BEHIND_MS, local),
        Skew::Ok
    );
    assert_eq!(
        Skew::check(local + MAX_PEER_SKEW_AHEAD_MS + 1, local),
        Skew::Ahead(MAX_PEER_SKEW_AHEAD_MS + 1)
    );
    assert_eq!(
        Skew::check(local - MAX_PEER_SKEW_BEHIND_MS - 1, local),
        Skew::Behind(MAX_PEER_SKEW_BEHIND_MS + 1)
    );
    assert_eq!(
        Skew::check(u64::MAX, 0),
        Skew::Ahead(u64::MAX),
        "no overflow at the extremes"
    );
}

#[test]
fn errors_name_both_clocks_and_the_bound() {
    let e = HlcError::PeerAhead {
        peer_ms: 700_000,
        local_ms: 100_000,
        bound_ms: MAX_PEER_SKEW_AHEAD_MS,
    };
    let text = e.to_string();
    assert!(text.contains("700000") && text.contains("100000"), "{text}");
    assert!(text.contains("600000") && text.contains("300000"), "{text}");
    assert_eq!(
        HlcError::Overflow { wall_ms: 5 }.to_string(),
        "hlc counter overflow at wall_ms 5"
    );
}

proptest! {
    #[test]
    fn ticks_are_strictly_increasing(nows in proptest::collection::vec(0u64..10, 1..200)) {
        let mut clock = Hlc::zero(dev(7));
        let mut prev = clock;
        for now in nows {
            let next = clock.tick(now).unwrap();
            prop_assert!(next > prev);
            prev = next;
        }
    }

    #[test]
    fn order_is_wall_then_counter_then_device(a: (u64, u16, u128), b: (u64, u16, u128)) {
        let x = hlc(a.0, a.1, a.2);
        let y = hlc(b.0, b.1, b.2);
        prop_assert_eq!(x.cmp(&y), (a.0, a.1, a.2).cmp(&(b.0, b.1, b.2)));
        let bytes = postcard::to_allocvec(&x).unwrap();
        prop_assert_eq!(postcard::from_bytes::<Hlc>(&bytes).unwrap(), x);
    }

    /// Merging a then b and b then a both land above self, a and b (the invariant, not equality).
    #[test]
    fn merge_order_does_not_matter_for_dominance(
        local in (0u64..1_000_000, 0u16..100),
        a in (0u64..1_000_000, 0u16..100),
        b in (0u64..1_000_000, 0u16..100),
        now_ms in 0u64..1_000_000,
    ) {
        let start = hlc(local.0, local.1, 1);
        let (a, b) = (hlc(a.0, a.1, 2), hlc(b.0, b.1, 3));
        // Keep both peers inside the ahead bound so neither order is refused for skew.
        prop_assume!(a.wall_ms <= now_ms + MAX_PEER_SKEW_AHEAD_MS);
        prop_assume!(b.wall_ms <= now_ms + MAX_PEER_SKEW_AHEAD_MS);
        let mut ab = start;
        ab.merge(a, now_ms).unwrap();
        let ab = ab.merge(b, now_ms).unwrap();
        let mut ba = start;
        ba.merge(b, now_ms).unwrap();
        let ba = ba.merge(a, now_ms).unwrap();
        for got in [ab, ba] {
            prop_assert!(got > start && got > a && got > b);
            prop_assert_eq!(got.device, start.device);
        }
    }
}
