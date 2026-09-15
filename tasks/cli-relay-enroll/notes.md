# `txtodo relay enroll`: bind this device's relay node id to an OTP-verified email

## Goal

The one human-facing step of ADR 0027 phase 2. Two invocations: ask for a code, then present it
along with this device's relay node id. After that the daemon connects exactly as before.

## The key property: nothing about sync changes

The relay gates on the node id it authenticates during its own handshake. So enrollment is a
**side-channel registration**, not a change to how the daemon connects — no new header, no token in
`RelayConfig`, no protocol version bump, no crypto. `crates/txtodo-sync/src/relay.rs` is untouched
by this task.

(Worth knowing, not needed here: iroh's client *does* support a relay auth token —
`RelayConfig::with_auth_token`, sent as `Authorization: Bearer` on the WebSocket upgrade. That is
the path ADR 0027 rejected. If the shared-token design is ever revisited, that one line is where it
would go.)

## Depends on

`cli-relay-node-id` — this command cannot post an id the daemon won't tell it. Do that first.

## Design notes

- **Precedence for `accounts_url`** mirrors `relay_url` exactly (`crates/txtodo-cli/src/config.rs`'s
  `resolve_relay_url`: flag > env > config > `None`). Absence stays `None`; enrollment is opt-in the
  same way relay is.
- **Don't take the code as a bare argument only.** `crates/txtodo-cli/src/bundle.rs` already
  establishes the prompt-rather-than-arg pattern for a secret. A 6-digit code is short-lived and
  low-value, but the habit is cheap and the flag is still there for scripts.
- **Guard on "no relay configured".** Otherwise the command succeeds, the human believes they are
  enrolled, and nothing ever connects — the worst kind of failure, because it looks like the server
  is broken.
- **Doctor is the diagnostic surface.** A relay refusing a connection produces
  `Access::Deny { reason: None }` on the server and a generic failure on the client — iroh does not
  carry a reason back. Without a doctor line saying "this device is not enrolled", a denied device is
  genuinely hard to tell apart from a network problem. This is the single most valuable item in this
  task.

## Acceptance

The test in `todo.txt`, against a stub accounts server (not the real one — this is a CLI test).
Plus, by hand at deploy time: enroll a third machine, watch it start syncing, revoke it in the
accounts DB, watch it stop.

## Not in scope

Account recovery, changing the email on an account, and self-serve signup. All manual database
operations for now, matching ADR 0018's "manually granted" decision.
