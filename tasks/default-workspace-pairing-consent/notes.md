# default-workspace-pairing-consent

Found reviewing the last 100 commits (`18b37ea^..HEAD`) on 2026-09-21.

## The question

ADR 0029 gives every device's default workspace the same compile-time reserved ULID
(`crates/txtodo-daemon/src/default_workspace.rs:33-38`) — that is what makes it converge across
your own devices whatever the OS path, and `1e096c3` tests exactly that.

Sync is keyed by workspace id with nothing negotiated
(`workspace_catalog_offers.rs:113-118`). Every *other* workspace reaches a peer through
`Offer` → a human `accept_offer` / `decline_offer`. The default bypasses that entirely.

So: pair once with anyone — a colleague, to share one project list — and your private default list
unions with theirs immediately and irreversibly. No consent prompt, no opt-out.

This is a design decision, not a bug to go fix. ADR 0029 chose the reserved id deliberately; it did
not address pairing with a device that is not yours. Options as I see them:

- **A.** The reserved id only converges between devices that share an identity. Pairing with a
  foreign device offers the default like any other workspace.
- **B.** The default never syncs over a pairing at all; it converges only through a
  same-account/same-identity path (which does not exist yet — see `tasks/relay-id-keystore`).

A is less work and keeps today's multi-device behaviour. B is safer and pushes the problem into
identity, where it arguably belongs. Either way this needs a human call and probably an amendment
to ADR 0029 rather than a new ADR.

## Second, smaller thing

`crates/txtodo-daemon/src/pairing_grpc.rs:44-49`: `resolve(None)` now returns the default
workspace, so a selector-less `PairAccept` lands on it; `adopt_offered_workspace_id` then returns
`current_id` and the caller discards the value. The user scanned a code naming workspace X, sees a
successful `PairResult`, and X never appears — it arrives later as a pending offer they must
separately accept. The only record is a `tracing::info!`. `PairResult` should carry that the
offered id was not adopted, or the RPC should refuse a selector-less accept.

That one is independent of the decision above and can ship either way.

The reserved-id guard itself (`9116a6a`, "PairAccept never rekeys the default workspace off its
reserved id") is correct, and the reverse collision is caught by `registry.adopt`. The gap is the
missing consent step, not the guard.
