//! Relay/forced-relay test helpers (plan M8 `relay-converge-test`), split out of `mod.rs` purely
//! for that file's line budget — same pattern this crate's own `src/` uses throughout (e.g.
//! `workspace_error.rs` out of `workspace.rs`). A child of `support`, so it reaches `Daemon`'s
//! private fields and `start_in` the same way `mod.rs` itself does (Rust privacy: visible to the
//! defining module and every descendant).

use super::{Daemon, seed_group_id, write_tree};

/// `start_with_seeded_group_tree`, plus extra `txtodod` CLI args appended after the standard
/// `--dir`/`--identity-mode` ones — `--relay`, `--relay-dial-peer`, `--no-lan` (`relay.rs`'s own
/// module doc on the daemon side explains why `--relay-dial-peer` exists at all: no
/// pairing-over-relay yet, so nothing else tells a daemon who to reach across a real network
/// boundary).
pub async fn start_with_seeded_group_args(
    files: &[(&str, &str)],
    mode: &str,
    group_id: u128,
    extra_args: &[String],
) -> Daemon {
    let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
    write_tree(dir.path(), files);
    seed_group_id(dir.path(), group_id);
    Daemon::start_in(dir, mode, &[("TXTODO_TEST_HOOKS", "1")], extra_args).await
}

/// `relay.rs`'s `bind()` records `"bound as <hex>; awaiting connections"` in
/// `Health.relay_last_outcome` on a successful bind — this parses the hex node id back out, so one
/// daemon's relay identity can be handed to another's `--relay-dial-peer` without any pairing
/// protocol. `None` until the relay endpoint has actually bound (poll, do not assume readiness).
pub fn parse_relay_node_id(relay_last_outcome: &str) -> Option<String> {
    let hex = relay_last_outcome
        .strip_prefix("bound as ")?
        .split(';')
        .next()?;
    (hex.len() == 64 && hex.bytes().all(|b| b.is_ascii_hexdigit())).then(|| hex.to_owned())
}

#[cfg(test)]
mod tests {
    use super::parse_relay_node_id;

    #[test]
    fn parses_the_bound_outcome() {
        let hex = "a".repeat(64);
        let outcome = format!("bound as {hex}; awaiting connections");
        assert_eq!(parse_relay_node_id(&outcome), Some(hex));
    }

    #[test]
    fn none_before_bound() {
        assert_eq!(parse_relay_node_id(""), None);
        assert_eq!(parse_relay_node_id("bind failed: x"), None);
    }
}
