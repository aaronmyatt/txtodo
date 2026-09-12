//! want(): exactly the ops the peer holds and we do not; advance(): heads move only contiguously.

use std::collections::{BTreeMap, BTreeSet};

use crate::message::{Heads, MAX_WANT_RANGES, OriginRange};
use crate::want::{Gap, advance, want};
use proptest::prelude::*;
use txtodo_model::{DeviceId, Ulid};

fn dev(n: u128) -> DeviceId {
    DeviceId::new(Ulid::from_u128(n))
}

fn heads(pairs: &[(u128, u64)]) -> Heads {
    pairs.iter().map(|&(d, h)| (dev(d), h)).collect()
}

fn range(device: u128, first: u64, last: u64) -> OriginRange {
    OriginRange {
        device: dev(device),
        first,
        last,
    }
}

#[test]
fn want_asks_for_the_runs_past_our_heads_and_nothing_the_peer_lacks() {
    let local = heads(&[(1, 5), (2, 3)]);
    let remote = heads(&[(1, 7), (3, 2)]);
    assert_eq!(
        want(&local, &remote),
        vec![range(1, 6, 7), range(3, 1, 2)],
        "device 2 is ours alone; device 3 starts from 1"
    );
    assert!(want(&remote, &remote).is_empty(), "equal heads are in sync");
    assert!(
        want(&heads(&[(1, 9)]), &heads(&[(1, 4)])).is_empty(),
        "a peer behind us wants nothing from us here"
    );
    assert!(want(&Heads::new(), &Heads::new()).is_empty());
}

#[test]
fn want_is_bounded_and_the_rest_waits_for_the_next_round() {
    let remote: Heads = (0..(MAX_WANT_RANGES as u128 + 5))
        .map(|d| (dev(d), 1))
        .collect();
    let got = want(&Heads::new(), &remote);
    assert_eq!(got.len(), MAX_WANT_RANGES);
    assert_eq!(got[0].device, dev(0), "device order, lowest first");
    let caught_up: Heads = got.iter().map(|r| (r.device, r.last)).collect();
    assert_eq!(want(&caught_up, &remote).len(), 5, "the tail comes next");
}

#[test]
fn advance_moves_a_head_only_by_a_run_that_follows_it() {
    let mut h = heads(&[(1, 5)]);
    advance(&mut h, &range(1, 6, 8)).unwrap();
    assert_eq!(h, heads(&[(1, 8)]));
    advance(&mut h, &range(2, 1, 1)).unwrap();
    assert_eq!(h, heads(&[(1, 8), (2, 1)]), "an unknown device starts at 0");
    let hole = range(1, 10, 11);
    assert_eq!(
        advance(&mut h, &hole),
        Err(Gap {
            device_head: 8,
            range: hole
        })
    );
    let repeat = range(1, 8, 9);
    assert!(matches!(advance(&mut h, &repeat), Err(Gap { .. })));
    assert_eq!(h, heads(&[(1, 8), (2, 1)]), "a refused run changes nothing");
}

fn head_map() -> impl Strategy<Value = Heads> {
    proptest::collection::btree_map(0u128..6, 0u64..8, 0..6)
        .prop_map(|m: BTreeMap<u128, u64>| m.into_iter().map(|(d, h)| (dev(d), h)).collect())
}

fn expand(ranges: &[OriginRange]) -> BTreeSet<(DeviceId, u64)> {
    ranges
        .iter()
        .flat_map(|r| (r.first..=r.last).map(move |s| (r.device, s)))
        .collect()
}

proptest! {
    /// The invariant: want(local, remote) names exactly the (device, seq) pairs remote holds and
    /// local does not — no more, no fewer.
    #[test]
    fn want_requests_exactly_the_missing_ops(local in head_map(), remote in head_map()) {
        let expected: BTreeSet<(DeviceId, u64)> = remote
            .iter()
            .flat_map(|(d, &their)| {
                let ours = local.get(d).copied().unwrap_or(0);
                (ours + 1..=their).map(move |s| (*d, s))
            })
            .collect();
        let got = want(&local, &remote);
        prop_assert_eq!(expand(&got), expected);
        prop_assert!(got.iter().all(|r| r.first <= r.last));
        // Committing every run in order brings local up to remote for every device remote knows.
        let mut caught_up = local.clone();
        for r in &got {
            advance(&mut caught_up, r).unwrap();
        }
        prop_assert!(want(&caught_up, &remote).is_empty());
    }
}
