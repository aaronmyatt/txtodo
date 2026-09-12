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
}
