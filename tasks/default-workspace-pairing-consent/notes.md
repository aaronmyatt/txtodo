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

## Decided 2026-09-24 (human): A

- Pairing asks "is this your own device?" (CLI `pair`, TUI, desktop). The answer rides the
  handshake and is stored on the `devices` row.
- The default workspace merges by its reserved id only when **both** sides marked each other as
  own-device. Mismatched answers take the stricter reading: no merge.
- Any other peer gets the default offered like a normal workspace. Under
  `ref:remote-workspace-mirror` (same day) that means it lands as a separate Remote mirror at
  `<data dir>/remote/<workspace-id>/`. Your own default never merges into theirs.
- Migration: devices paired before this ships count as own-device. Pairing with a foreign device
  has not been a supported flow, so that is today's truth.
- ADR 0029 needs a short amendment: the reserved id converges only between own devices.
- Open, not decided: can a user flip the flag after pairing? Not asked; leave it off until wanted.

## Build plan (2026-09-24, agent)

- Question at SAS time: `PairConfirmRequest.own_device`. The joiner's answer rides `JoinerHello`
  (new field); the initiator's rides the sealed `PairingGrant` (new field). Each side stores
  `own = mine && theirs` on the peer's `devices` row. Wire note: postcard refuses a struct with a
  field missing or extra, so a pre-change build and a post-change build can no longer pair with
  each other. Devices pair at the same version in practice (clients upgrade the daemon), so this
  is noted, not versioned.
- Store: `devices.own_device` column; the migration marks every existing row own-device (the
  decided migration: pairing a foreign device was never a supported flow).
- Merge gate: a sync session learns the peer from its link `Hello`. It now sends its `Greet`s only
  after that, and drops the reserved default from the session's routes unless the peer's row says
  own-device.
- The foreign default as a Remote entry: both devices hold the reserved id for their *own*
  default, so a mirror of the other one needs its own id. Each device offers its default under an
  alias, `alias(device) = blake3("txtodo default alias" || reserved id || device id)` cut to a ULID
  with a non-zero timestamp. A session with a non-own peer routes `alias(self)` to this device's
  default. The receiver mirrors `alias(sender)` like any offer; an own-device receiver skips it
  (it already merges that list under the reserved id). Offering the plain reserved id stops.
- `PairResult.kept_own_workspace`: true when the joiner kept its default instead of adopting the
  offered id; the CLI then says the other device's workspace arrives as a Remote entry.
- TUI: it has no pairing flow, so there is nothing to ask there.
