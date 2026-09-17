# pairing-workspace-identity

`txtodo pair` agrees on a group id and a group key. It never agrees on a `WorkspaceId`. Since
`daemon-workspace-session-multiplex` stages 1–2 made every sync message carry one, that means two
freshly-paired devices pair successfully and then never sync a single byte.

Filed 2026-09-16, from a CI failure that had been mislabelled a flake.

## The bug, as observed

`crates/txtodo-cli/tests/pairing.rs`'s
`two_real_devices_complete_pairing_and_the_joiner_receives_the_initiators_real_file` fails
deterministically — ubuntu-latest, macos-latest and locally on macOS, every run. Pairing itself
completes: the QR/code preamble renders, both sides derive the same real six-word SAS, both
confirm, the joiner prints `Paired.` and the group key lands. Then the joiner's `todo.txt` stays
empty until the 30 s deadline.

Both daemons' own `TXTODO_LOG=debug` JSON logs say why, in as many words:

```
A: lan_shared_session_started {"workspaces":1}
A: lan_link_hello_accepted {"skew":"Ok"}
A: lan_session_unrouted_workspace_message_skipped {"workspace":"01M2M33FTPQR1QKXJRD2BSHNNS"}

B: lan_shared_session_started {"workspaces":1}
B: lan_link_hello_accepted {"skew":"Ok"}
B: lan_session_unrouted_workspace_message_skipped {"workspace":"01M2M33FTPC02YNM2FT335JYWK"}
```

Two different workspace ids for what is, to the human, one shared workspace. The connection is
real, the link-level `Hello` is accepted on both sides, and then each side drops the other's every
`Greet` and reconnects, forever.

## Why

- A `WorkspaceId` is a ULID minted locally, per device: `txtodo_store::registry::WorkspaceId`
  ("a minted ULID, never the path itself"), handed out by `workspace_registry::add` from
  `clock.new_ulid()`. Nothing about it is derived from the group, the path, or anything two devices
  could independently agree on.
- `lan_session_dispatch.rs::dispatch_workspace_frame` resolves each incoming frame's peeked
  workspace id against this side's own `WorkspaceRoutes`, and a workspace this side never opened is
  skipped — deliberately non-fatal, per `lan_session_shared.rs`'s "failure scope". With mismatched
  ids, *every* workspace frame takes that branch.
- Pairing never touches workspace identity. The daemon crate's own test support says so outright,
  in `Daemon::start_with_workspace_id`'s doc: *"workspace identity is still a separate dimension
  pairing does not touch — both sides need the same id before any post-pairing sync can open."*

## Why no other test catches it

Every other real-daemon sync test pre-seeds **both** sides with the same id and so pre-agrees the
exact thing that is broken:

| Test | How it agrees |
|---|---|
| `txtodo-daemon/tests/pairing_lan.rs` | `Daemon::start_with_workspace_id(..., workspace_id)` on both sides |
| `txtodo-daemon/tests/pairing_relay.rs` | `support::relay::start_with_seeded_group_args`, seeded id |
| `txtodo-daemon/tests/relay_multiplex.rs` | `seed_workspace_at(&registry_db, root, ws_id)` per side |

The CLI test is the only one that pairs two genuinely independent daemons and then asks whether
sync converges. It is the honest one, and it is the one that fails.

## The fix

The machinery already exists — `daemon-workspace-identity-agreement` (root todo 163) landed all
seven stages of an offer/accept workspace-identity exchange, first-registrant-wins:

- `WorkspaceRegistry::add_with_id` / `adopt` — register under a caller-supplied id rather than
  minting one, with typed `IdCollision`/`RootCollision` refusals in both directions.
- `txtodo_sync::control`'s `ControlMessage::{Offer, OfferAck, Decline}`, sealed with the group-key
  AEAD, carried over `CONTROL_ALPN`.
- `control_channel.rs` / `control_session.rs` — the always-on device-level control channel.
- `workspace_offer_grpc.rs` — `WorkspacePendingOffers` / `WorkspaceAcceptOffer` /
  `WorkspaceDeclineOffer`.

What that task deliberately did **not** build, in its own words (its `todo.txt`, stage 6):

> CLI commands (`txtodo workspace offers`/`accept`) not built — not required for stage 7's payoff
> (the AEAD re-bind), left as a real follow-up for whoever wires the human-facing CLI surface, not
> silently dropped.

This task is that follow-up, plus the part nobody wrote down: pairing should drive the exchange
itself, so a human who runs `txtodo pair` ends up with a working sync and never has to know the
word "workspace id".

## Open design questions

1. **Does pairing drive it, or does the human?** Pairing already has both devices' attention and a
   confirmed SAS — adopting the initiator's workspace id there needs no new human step. The
   alternative (pair, then `txtodo workspace offers` / `accept`) is more explicit and composes with
   adding a *second* workspace to an existing pair, which pairing can't help with. Probably both:
   pairing does the first workspace, the CLI commands handle the rest.
2. **Which carrier?** The control channel is device-level and relay-backed
   (`CONTROL_ALPN` over one `RelayEndpoint` per device). `crates/txtodo-cli/tests/pairing.rs` pairs
   over the LAN with no relay configured at all, so either the exchange needs a LAN route too, or
   pairing carries the id inline in the existing `PairOffer`/`PairAccept` handshake it already
   runs. The latter is much smaller; it is also a wire-format change to a security-sensitive
   handshake, so it wants a real decision, not a guess.
3. **Collision handling at pairing time.** `adopt` already refuses both collision directions. What
   should `txtodo pair` *print* when the joiner's directory is already registered under a different
   id? Refusing is right; the message needs to tell the human what to do next.

## Acceptance

Un-`#[ignore]` `crates/txtodo-cli/tests/pairing.rs`'s
`a_paired_joiner_receives_the_initiators_real_file` and have it pass on all three runners. That
test was quarantined by this task's filing and is its acceptance bar; nothing else needs to be
written to prove the fix.

## As built

The initiator's real `WorkspaceId` now rides `PairOfferResponse` as a ninth field (catalog
metadata, same treatment as `identity_mode` — not part of the crypto transcript); the joiner
adopts it via `WorkspaceCatalog::adopt_offered_workspace_id` (first-registrant-wins: releases its
self-registered row, adopts the offered id, rekeys the open-map entry, live `Workspace` id, and
device-level relay/file-carrier routes together).

`crates/txtodo-cli/tests/pairing.rs::a_paired_joiner_receives_the_initiators_real_file` un-ignored
and green (4/4 local repeat runs), no regressions in `txtodo-daemon`'s 213 lib tests or the real
two-daemon `pairing_lan.rs` test.

Found and fixed a real dependency along the way: `txtodo-store`'s `heads()`/`head_of()`/
`next_origin_seq()`/`put_projection()`/`put_snapshot()` exceeded the cognitive-complexity budget
once instrumented (pre-existing, from the M11 logging milestone) — split with the same
thin-wrapper-plus-`_inner` pattern as `OpenedWorkspace::drop`/`drop_inner`. The rest of that
pre-existing debt (at least `txtodo-crdt`) is out of scope here, flagged as its own follow-up.

**Deliberately still open, genuinely separate scope**: `txtodo workspace offers`/`accept` CLI
surface for adding a second workspace to an already-paired device (this task's own `todo.txt` item
4); not verified on ubuntu/Windows CI (no CI access this session, no platform-specific code path
involved).
