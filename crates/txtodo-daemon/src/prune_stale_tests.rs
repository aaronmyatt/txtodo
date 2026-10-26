//! The stale verdict (task `peer-prune`, ADR 0044 decision 2): parked and quiet peers, each rule's
//! `0` off switch, the grace after this device was away, and the duration words.

use txtodo_model::{DeviceId, Ulid};
use txtodo_store::DeviceRow;

use super::{DAY_MS, PruneConfig, Stale, parse_duration_ms, stale_peers, verdict};
use crate::peer_keys::{PARK_AFTER, SessionEnd, WRONG_GROUP};
use crate::peer_keys_dial_tests::identity_in;

fn row(last_seen_ms: Option<u64>, paired_at_ms: u64) -> DeviceRow {
    DeviceRow {
        device: DeviceId::new(Ulid::from_u128(7)),
        name: String::new(),
        static_public: [0; txtodo_store::DEVICE_STATIC_KEY_BYTES],
        paired_at_ms,
        last_seen_ms,
        last_known_wall_ms: None,
        key_epoch: 0,
        removed_at_ms: None,
        relay_node_id: None,
        relay_url: None,
    }
}

const NOW: u64 = 30 * DAY_MS;

#[test]
fn a_peer_heard_recently_is_not_stale() {
    let cfg = PruneConfig::default();
    let heard = row(Some(NOW - DAY_MS / 2), 0);
    assert_eq!(verdict(&heard, false, &cfg, NOW, 0), None);
    assert_eq!(
        verdict(&heard, true, &cfg, NOW, 0),
        None,
        "parked, but not a day yet"
    );
}

#[test]
fn a_quiet_peer_is_stale_after_the_quiet_threshold_and_not_before() {
    let cfg = PruneConfig::default();
    let just_short = row(Some(NOW - 7 * DAY_MS + 1), 0);
    let exactly = row(Some(NOW - 7 * DAY_MS), 0);
    assert_eq!(verdict(&just_short, false, &cfg, NOW, 0), None);
    assert_eq!(verdict(&exactly, false, &cfg, NOW, 0), Some(Stale::Quiet));
}

#[test]
fn a_parked_peer_is_stale_after_the_parked_threshold() {
    let cfg = PruneConfig::default();
    let silent_two_days = row(Some(NOW - 2 * DAY_MS), 0);
    assert_eq!(
        verdict(&silent_two_days, true, &cfg, NOW, 0),
        Some(Stale::Parked)
    );
    assert_eq!(
        verdict(&silent_two_days, false, &cfg, NOW, 0),
        None,
        "not parked: still quiet only"
    );
}

#[test]
fn zero_turns_a_rule_off() {
    let silent_ten_days = row(Some(NOW - 10 * DAY_MS), 0);
    let no_quiet = PruneConfig {
        quiet_after_ms: 0,
        ..PruneConfig::default()
    };
    assert_eq!(verdict(&silent_ten_days, false, &no_quiet, NOW, 0), None);
    assert_eq!(
        verdict(&silent_ten_days, true, &no_quiet, NOW, 0),
        Some(Stale::Parked)
    );
    let no_parked = PruneConfig {
        parked_after_ms: 0,
        ..PruneConfig::default()
    };
    assert_eq!(
        verdict(&silent_ten_days, true, &no_parked, NOW, 0),
        Some(Stale::Quiet),
        "a parked peer past the quiet threshold is still quiet"
    );
}

#[test]
fn a_peer_never_heard_is_counted_from_when_it_was_paired() {
    let cfg = PruneConfig::default();
    let fresh = row(None, NOW - DAY_MS);
    let old = row(None, NOW - 8 * DAY_MS);
    assert_eq!(verdict(&fresh, false, &cfg, NOW, 0), None);
    assert_eq!(verdict(&old, false, &cfg, NOW, 0), Some(Stale::Quiet));
}

#[test]
fn time_this_device_was_away_does_not_count_against_a_peer() {
    let cfg = PruneConfig::default();
    let silent = row(Some(NOW - 20 * DAY_MS), 0);
    assert_eq!(verdict(&silent, false, &cfg, NOW, 0), Some(Stale::Quiet));
    let back_an_hour_ago = NOW - 3_600_000;
    assert_eq!(verdict(&silent, false, &cfg, NOW, back_an_hour_ago), None);
    assert_eq!(verdict(&silent, true, &cfg, NOW, back_an_hour_ago), None);
}

#[test]
fn a_removed_peer_has_no_verdict() {
    let cfg = PruneConfig::default();
    let mut gone = row(Some(0), 0);
    gone.removed_at_ms = Some(1);
    assert_eq!(verdict(&gone, true, &cfg, NOW, 0), None);
}

#[test]
fn stale_peers_reads_the_table_and_the_parked_flag() {
    let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
    let identity = identity_in(dir.path(), [7; 32]);
    let device = |n: u128| DeviceId::new(Ulid::from_u128(n));
    let (quiet, parked, fine) = (device(11), device(12), device(13));
    for (peer, seen) in [(quiet, 0), (parked, NOW - 2 * DAY_MS), (fine, NOW - 1_000)] {
        let new = txtodo_store::NewDevice {
            device: peer,
            name: String::new(),
            static_public: [0; txtodo_store::DEVICE_STATIC_KEY_BYTES],
            paired_at_ms: seen,
            last_known_wall_ms: None,
            key_epoch: 0,
        };
        let mut store = identity.store().lock().unwrap_or_else(|e| e.into_inner());
        store
            .register_device_as(&new, true)
            .unwrap_or_else(|e| panic!("register: {e}"));
    }
    for _ in 0..PARK_AFTER {
        identity
            .peer_keys()
            .book_session(Some(parked), SessionEnd::Refused(WRONG_GROUP));
    }
    let mut stale = stale_peers(&identity, &PruneConfig::default(), NOW, 0);
    stale.sort_by_key(|(device, _)| *device);
    assert_eq!(stale, vec![(quiet, Stale::Quiet), (parked, Stale::Parked)]);
}

#[test]
fn durations_read_as_digits_and_a_unit() {
    assert_eq!(parse_duration_ms("0"), Some(0));
    assert_eq!(
        parse_duration_ms("45"),
        Some(45_000),
        "bare digits are seconds"
    );
    assert_eq!(parse_duration_ms("30s"), Some(30_000));
    assert_eq!(parse_duration_ms("90m"), Some(5_400_000));
    assert_eq!(parse_duration_ms("12h"), Some(43_200_000));
    assert_eq!(parse_duration_ms(" 7d "), Some(7 * DAY_MS));
    for bad in [
        "",
        "d",
        "-1d",
        "1.5d",
        "1w",
        "1 day",
        "18446744073709551615d",
    ] {
        assert_eq!(parse_duration_ms(bad), None, "{bad:?}");
    }
}
