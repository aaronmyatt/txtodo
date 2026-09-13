//! The relay-on twin of `endpoint_tests.rs`'s "single most useful test": proves the configured
//! relay set is non-empty and holds exactly the configured URL, so an iroh upgrade that changes
//! how `RelayMode::Custom` is built fails here, not silently. Asserted on `relay_map()` — the
//! validated, pre-bind `RelayMap` — rather than a bound endpoint's `home_relay_status()`, which
//! only reports a *live* connection outcome and stays empty forever without a reachable relay
//! (there is no relay server in this test, or in CI); this needs no network and cannot hang.

use iroh::RelayUrl;

use crate::relay::{MAX_RELAY_PEERS, RelayConfig, RelayError, build_endpoint, relay_map};

fn cfg(url: &str) -> RelayConfig {
    RelayConfig {
        url: url.to_string(),
        max_peers: 1,
    }
}

#[tokio::test]
async fn empty_url_is_refused_before_binding() {
    let err = build_endpoint(&cfg("")).await.unwrap_err();
    assert!(matches!(err, RelayError::EmptyUrl));
}

#[tokio::test]
async fn invalid_url_is_refused() {
    let err = build_endpoint(&cfg("not a url")).await.unwrap_err();
    assert!(matches!(err, RelayError::InvalidUrl(_)));
}

#[tokio::test]
async fn max_peers_over_the_cap_is_refused() {
    let mut c = cfg("https://relay.example.org");
    c.max_peers = MAX_RELAY_PEERS + 1;
    let err = build_endpoint(&c).await.unwrap_err();
    assert!(
        matches!(err, RelayError::TooManyPeers { requested } if requested == MAX_RELAY_PEERS + 1)
    );
}

#[test]
fn a_valid_relay_url_produces_a_non_empty_configured_relay_set() {
    let url: RelayUrl = "https://relay.example.org".parse().unwrap();
    let map = relay_map(&cfg("https://relay.example.org")).unwrap();
    let urls: Vec<RelayUrl> = map.urls();
    assert_eq!(
        urls,
        vec![url],
        "relay must be on by construction, not by omission"
    );
}
