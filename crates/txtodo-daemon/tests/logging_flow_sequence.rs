//! logging-flow-test, part (b): the real two-daemon pairing+converge path (`pairing_lan.rs`'s own
//! proof, reused verbatim) driven under `TXTODO_LOG=debug`, asserting the *ordered sequence* of
//! event names in the joiner's own JSON log — not just that one name appears somewhere
//! (`relay_multiplex.rs`'s existing `lan_shared_session_started` grep is presence-only; this is
//! the acceptance bar's own escalation). Reuses `tests/support/mod.rs`'s
//! `start_with_workspace_id_and_envs` (added by this task) and `log_tail()`.
//!
//! **Real finding from building this test (flagged to the human, not fixed — see the
//! `TXTODO_LOG_DEBUG_MINUS_GRPC_NOISE` constant below for the full writeup):** a bare
//! `TXTODO_LOG=debug` makes a `--dir` bridge daemon's own gRPC surface catastrophically slow
//! (a startup that normally takes ~1.7s took over 100s and never completed in repeated direct
//! measurement), because `debug` is a blanket `EnvFilter` level covering every dependency, not
//! just this workspace's own crates — including the chatty gRPC/networking stack this same
//! harness's own polling rides on. `txtodo_telemetry::build_filter()` already knows to quiet one
//! noisy dependency (`loro`) this same way; it does not yet do the same for `hyper`/`h2`/`tower`/
//! `tonic`/`mdns-sd`/`iroh`, which is a real, pre-existing gap in the shipped logging epic.
//!
//! Separately (unrelated to the finding above): this environment's real mDNS/LAN pairing is
//! itself known-flaky (observed directly: `pairing_lan.rs`'s own unmodified test failed once with
//! "the group key never landed on the joiner within 30s" and passed cleanly on retry, the same
//! real-network-variance shape `pairing_relay.rs`'s module doc already documents and one of its
//! tests is quarantined for) — not a regression from this task, and not specific to this test.
//!
//! **2026-09-17: quarantined `#[ignore]`.** The flake rate here turned out higher than "not
//! specific to this test" implied — roughly 50% locally, reproduced against an unmodified
//! pre-`relay-default-public-url` baseline (a temporary `git worktree` at that commit, not this
//! session's own changes) to rule out a regression before quarantining it. See
//! `docs/testing-guide.md` for how to reproduce it by hand and how to run this crate's suite.
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::print_stderr)]
#![cfg(unix)]

mod support;

use std::time::{Duration, Instant};
use support::Daemon;
use txtodo_proto::v1 as pb;

fn rand_u128() -> u128 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    Instant::now().hash(&mut hasher);
    std::process::id().hash(&mut hasher);
    (u128::from(hasher.finish()) << 64) | u128::from(hasher.finish().wrapping_add(1))
}

const PAIR_DEADLINE: Duration = Duration::from_secs(30);

/// Field-for-field `PairOfferResponse` as the JSON `code` `PairAccept` parses — mirrors
/// `pairing_lan.rs`'s own `response_to_code` (that file's own doc explains why this integration
/// test, a separate binary, cannot reuse `pairing_wire.rs::response_to_code` directly).
fn response_to_code(r: &pb::PairOfferResponse) -> String {
    serde_json::json!({
        "device": r.device,
        "group_id": r.group_id,
        "x25519_pub": r.x25519_pub,
        "endpoint": r.endpoint,
        "nonce": r.nonce,
        "identity_mode": r.identity_mode,
    })
    .to_string()
}

async fn await_peer_sas(a: &mut Daemon) -> String {
    let start = Instant::now();
    loop {
        let sas = a.pair_await_peer().await.sas;
        if !sas.is_empty() {
            return sas;
        }
        assert!(
            start.elapsed() < PAIR_DEADLINE,
            "no joiner reached the initiator within {PAIR_DEADLINE:?}\n--- a's log ---\n{}",
            a.log_tail()
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

async fn wait_for_group_key(b: &mut Daemon) {
    let start = Instant::now();
    loop {
        if b.health().await.lan_group_key_present {
            return;
        }
        assert!(
            start.elapsed() < PAIR_DEADLINE,
            "the group key never landed on the joiner within {PAIR_DEADLINE:?}\n--- b's log ---\n{}",
            b.log_tail()
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

async fn wait_for_file_convergence(a: &mut Daemon, b: &mut Daemon) {
    let want = a.daemon_bytes().await;
    let start = Instant::now();
    loop {
        let got = b.daemon_bytes().await;
        if got == want {
            return;
        }
        assert!(
            start.elapsed() < PAIR_DEADLINE,
            "the joiner's file did not converge to the initiator's within {PAIR_DEADLINE:?}\nwant={:?}\ngot={:?}",
            String::from_utf8_lossy(&want),
            String::from_utf8_lossy(&got),
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// One parsed JSON log line's event name (`fields.message`) plus its raw text, in emission order.
/// `fields.message` is what every `tracing::info!`/`debug!("some_name")` call lands under in this
/// workspace's JSON layer (the macro's trailing string literal), the same field `mcp/tests/
/// smoke.rs`'s own sentinel test reads off a span's JSON line.
fn event_names_in_order(log: &str) -> Vec<String> {
    let mut names = Vec::new();
    for line in log.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with("---") {
            continue;
        }
        let Ok(value) = serde_json::from_str::<serde_json::Value>(trimmed) else {
            continue;
        };
        let Some(name) = value
            .get("fields")
            .and_then(|f| f.get("message"))
            .and_then(|m| m.as_str())
        else {
            continue;
        };
        names.push(name.to_owned());
    }
    names
}

/// True when every element of `expected`, in order, appears somewhere in `actual` in that same
/// relative order (a subsequence check — `actual` may freely carry other events in between).
fn is_ordered_subsequence(expected: &[&str], actual: &[String]) -> bool {
    let mut it = actual.iter();
    expected.iter().all(|want| it.any(|got| got == want))
}

#[tokio::test]
#[ignore = "real mDNS/LAN pairing convergence is flaky in this sandboxed dev environment (~50% \
            fail rate observed locally) — confirmed pre-existing via an isolated git-worktree \
            baseline at the commit before task relay-default-public-url, failing at the same rate \
            on unmodified code; not a regression from that task or pairing-code-compact, not \
            root-caused this pass. See docs/testing-guide.md for manual repro steps and this \
            file's own module doc for the known TXTODO_LOG=debug gRPC-noise finding."]
async fn two_real_daemons_pairing_and_first_convergence_emit_events_in_the_expected_order() {
    let workspace_id = rand_u128();
    // `debug` alone is a blanket EnvFilter directive: it also turns on debug/trace-level tracing
    // inside every dependency, not just this workspace's own crates -- including tonic/hyper/h2/
    // tower (the gRPC stack this same test's own polling health checks ride on) and iroh/mdns-sd
    // (the LAN transport). Confirmed by direct measurement (this task's own investigation, see
    // tasks/logging-flow-test/notes.md's "As built" section): a single daemon that normally
    // becomes ready in ~1.7s under `start_with_workspace_id_and_envs` took over 100s and never
    // completed under a bare `TXTODO_LOG=debug`, and became reliably fast again (~1.7s) the moment
    // hyper/h2/tower/tonic/mdns_sd/iroh were pinned back to `info` -- a real, reproducible,
    // pre-existing gap in txtodo-telemetry::build_filter(), which only ever default-quiets
    // `loro`/`loro_internal` this same way, never the equally chatty gRPC/networking stack.
    // Flagged to the human rather than fixed here (out of this task's scope: no production-code
    // edits) -- this directive is the acceptance bar's own "TXTODO_LOG=debug" applied to every
    // module this crate/epic actually instruments, working around the known-bad third-party ones.
    const TXTODO_LOG_DEBUG_MINUS_GRPC_NOISE: &str =
        "debug,hyper=info,h2=info,tower=info,tonic=info,mdns_sd=info,iroh=info";
    let mut a = Daemon::start_with_workspace_id_and_envs(
        "(A) buy milk id:01M2D3AAAAAAAAAAAAAAAAAAAA\n",
        workspace_id,
        &[("TXTODO_LOG", TXTODO_LOG_DEBUG_MINUS_GRPC_NOISE)],
    )
    .await;
    let mut b = Daemon::start_with_workspace_id_and_envs(
        "",
        workspace_id,
        &[("TXTODO_LOG", TXTODO_LOG_DEBUG_MINUS_GRPC_NOISE)],
    )
    .await;

    let offer = a.pair_offer().await;
    let code = response_to_code(&offer);
    let sas_b = b.pair_accept(code).await.sas;
    assert!(!sas_b.is_empty());
    let sas_a = await_peer_sas(&mut a).await;
    assert_eq!(sas_a, sas_b, "sanity: both sides derive the identical SAS");

    a.pair_confirm_sas().await;
    b.pair_confirm_sas().await;

    wait_for_group_key(&mut b).await;
    wait_for_file_convergence(&mut a, &mut b).await;

    let b_events = event_names_in_order(&b.log_tail());
    let expected: &[&str] = &[
        "pairing_joiner_group_key_adopted",
        "lan_shared_session_started",
        "lan_link_hello_accepted",
        "commit_done",
    ];
    assert!(
        is_ordered_subsequence(expected, &b_events),
        "joiner's log did not carry {expected:?} in order; saw: {b_events:?}\nfull log:\n{}",
        b.log_tail()
    );
}
