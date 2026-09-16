# txtodo-daemon: log the ~24 sites where an error vanishes with no output (root todo.txt line 36)

## Goal

`txtodo-daemon`'s sync-transport code (`pairing_lan.rs`, `lan_session.rs`, `control_session.rs`,
`lan_session_shared.rs`, `file_carrier.rs`, `lan.rs`, `relay_fallback.rs`) has the same bug class
root todo 170 (`sync-pairing-relay`) already fixed by hand for one path (`pairing_lan.rs::attempt`'s
LAN dial, `log_lan_connect_failed`/`log_lan_round_no_reply`): a `Result`/`Option` collapses to
`None`/`continue`/`false` with no `tracing` call anywhere on the failure path. This task applies
that same fix to every other named site — never changing observable behavior (a `Rejected` stays
`Rejected`, a `Pending` stays `Pending`, a dropped connection stays dropped), only making the reason
visible.

## Design

### Severity convention

Matching this crate's existing precedent (`pairing_lan.rs`'s own `log_lan_connect_failed` at
`debug`, `lan_apply.rs`'s `lan_sync_ops_refused` at `warn`): routine/expected outcomes (a joiner not
reachable yet, a peer not sharing a workspace, a rotation-race key gap) stay `debug`; a real
protocol/crypto/store failure that indicates something is actually broken is `warn`; a failure that
silently strands another device in an unrecoverable retry loop (the `try_finalize_initiator`
downgrade) is `error`. Every event name is namespaced by module/site so a human can grep for the one
symptom they are chasing without reading the whole log.

### Sites, in priority order

1. **`pairing_lan.rs::process_hello`** (4 `InitiatorReply::Rejected` returns, all silent): split into
   `reject_no_active_session` (snapshot lookup failed — `debug`, routine for a stale/late retry),
   `reject_protocol_mismatch` (role/group/nonce mismatch — `warn`), `reject_peer_conflict` (a second
   device claiming an already-handshaken nonce — `warn`, names both device ids), and
   `reject_handshake_failed` (the crypto step itself refused — `warn`). Each is a one-macro-call leaf
   function returned directly (`return reject_x(...)`), so `process_hello`'s own dispatch never has a
   macro call inside a branch.
2. **`pairing_lan.rs::finalize_or_pending`**'s `try_finalize_initiator(...).unwrap_or(None)`: this
   collapses a genuine keystore/crypto `Err` into the same `None` as "not ready yet", downgrading to
   `InitiatorReply::Pending` either way — behavior unchanged, but the `Err` case now logs
   `pairing_initiator_finalize_failed` at `error` (`log_finalize_failed`) before falling through,
   since a joiner retrying forever against a permanently broken initiator is the worst case in this
   whole task.
3. **`lan_session.rs::fetch_group_key`/`single_epoch_keys`** and **`control_session.rs`'s own
   duplicate pair**: both collapse a keystore error, a missing key, a corrupt-length key, and (for
   `single_epoch_keys`) a `GroupKeys::insert` failure into one `None`, which every caller
   (`lan_session_dispatch.rs`, `file_carrier.rs`, `control_session.rs` itself) then logs as one flat
   `*_skipped_no_group_key` debug event. Fixed at the source rather than by changing the `Option`
   signature (which would force edits to `lan_session_dispatch.rs`, not a named file): each internal
   failure point now logs its own specific reason (`_keystore_error`/`_missing`/`_corrupt_length` at
   `warn`/`debug`/`warn`; `_insert_failed` at `warn`) before returning `None`, so every caller's
   existing generic skip is now preceded by the real cause in the log stream. `control_session.rs`
   additionally drops its own now-redundant top-level `log_no_group_key`/`control_channel_session_
   skipped_no_group_key` — the specific reason inside `fetch_group_key` supersedes it.
4. **`lan_session_shared.rs`**: `open_and_decode_logged`'s one `lan_session_open_failed` debug event
   (line ~148) already prints `error = %e` (a full `Display` string), but not a queryable `kind` —
   added a local `sync_error_kind`/`message_error_kind`/`crypto_error_kind` (mirroring
   `txtodo_sync::CryptoError::kind()`, which is `pub(crate)` to that crate only, so re-derived here
   rather than reached across the crate boundary) so the event carries a stable tag alongside the
   free-text message. `handle_greet`/`handle_want`/`handle_ops`'s three outbound `ctx.send(...)`
   calls (Want/Ops-batch/Ack) silently returned `false`/dropped a batch on failure — each now logs
   `lan_want_send_failed`/`lan_ops_send_failed`/`lan_ack_send_failed` at `warn` with the `SyncError`
   before ending the connection or skipping the batch.
5. **`file_carrier.rs::send_route`** (one send tick, five swallow points): `seal_message`'s two
   internal failures (`Message::encode`, `aead_seal`) now use `.inspect_err` (this file's own
   existing idiom in `open_and_decode`/`open_and_verify`) to log `file_carrier_encode_failed`/
   `file_carrier_seal_failed` at `warn` before returning `None`. `serve_want`'s `StoreError` now logs
   `file_carrier_serve_want_failed` (`warn`) instead of a bare early return. `carrier.send`'s
   `LinkError` now logs `file_carrier_send_failed` (`warn`) instead of `continue`ing silently. The
   `advance` gap (already commented as "best-effort, retried next tick") now logs
   `file_carrier_advance_gap_retrying_next_tick` at `debug` — kept at `debug` since the comment's own
   reasoning (a legitimate retry-safe race, not a bug) still holds; only its total silence changes.
   `tick`'s own two `fetch_group_key`/`single_epoch_keys` call sites need no separate fix — item 3
   above already makes the shared helper log internally.
6. **`lan.rs::spawn_resync_dial`**: `let _ = dial_and_spawn(...).await;` discarded the boolean
   outcome outright, unlike `spawn_dial`'s sibling which threads it into `record_dial_outcome`. The
   module doc is explicit that resync deliberately skips `DialState` bookkeeping (unconditional
   churn, not failure recovery) — so this does not add that bookkeeping, only a `debug!(peer, ok,
   "lan_resync_dial_outcome")` so the outcome is at least visible in the log stream.
7. **`relay_fallback.rs::lan_then_relay`**: fixed once, in the shared generic function, rather than
   at each of its two real callers (`lan.rs::dial_and_spawn`, `pairing_relay_dial.rs::joiner_round` —
   the latter not a named file) — since both always call it with LAN as `primary` and relay as
   `fallback` (the module's own doc: "Tries the primary (LAN) path... falls back to the relay path"),
   logging inside `lan_then_relay` itself covers "either call site" without touching an unnamed one.
   `Ok(Some(link))` now logs `lan_then_relay_carrier_won` (`carrier = "lan"`, `debug`) before
   returning; the fallback branch logs the same event with `carrier = "relay"` on success, or
   `lan_then_relay_both_carriers_failed` (`debug`) when neither produced a link.

### Never touched

`pairing_lan.rs::attempt`/`log_lan_connect_failed`/`log_lan_round_no_reply` — already fixed by root
todo 170, read for the pattern, not modified. `lan_session_dispatch.rs`, `pairing_relay_dial.rs`,
`lan_peers.rs`, `lan_apply.rs` — not named by the backlog line; item 3 and item 7 above were
specifically designed to fix their generic downstream logging *without* editing them, by fixing the
shared function/helper they all call instead.

## Placement

Every fix is a leaf function containing exactly one `tracing` macro call, matching this crate's own
established rule (`pairing_lan.rs`'s existing `log_lan_connect_failed` et al.): a bare macro call
directly inside a `match` arm or `if` branch costs real `clippy::cognitive_complexity` points on its
own, independent of `#[instrument]` — confirmed by `logging-sync-crate`'s "As built" notes. No
function touched by this task needs an `#[instrument]` wrapper+inner split: none of these sites were
already at the complexity budget, and every fix here only replaces a silent early-return with a call
to a same-shaped leaf function, never adding a new branch.

## Edge cases

- `try_finalize_initiator`'s `Err` case must keep returning `Pending` on the wire (not `Rejected`) —
  changing that would be a real protocol/behavior change no test currently covers, out of scope for
  a logging-only pass.
- `lan_session.rs`/`control_session.rs`'s duplicate `fetch_group_key`/`single_epoch_keys` pairs are
  intentionally *not* deduplicated into one shared helper — that would touch the (unnamed)
  `lan_session_dispatch.rs` call site's import and is a refactor, not a logging fix.
- `file_carrier.rs`'s `advance` gap and `lan.rs`'s resync dial outcome stay at `debug`, not `warn`:
  both are already-documented, expected races in a periodic-redial design, not failures.

## Acceptance

- Every site named in the backlog line logs its specific outcome at a level matching this crate's
  existing conventions, with no payload/key material/line text in any new event (ids, counts, kinds,
  hashes only).
- `cargo fmt -p txtodo-daemon -- --check`, `cargo clippy -p txtodo-daemon --all-targets -- -D
  warnings`, `cargo test -p txtodo-daemon` green after every commit, no `#[allow]`/`#[expect]`
  anywhere.
- `pairing_lan_tests.rs` gets a new whitebox test proving the `Rejected` paths (previously untested
  as well as unlogged) still return the right wire reply; every other site is proven by its existing
  test suite staying green with the new logging compiled in (this crate installs no in-process
  subscriber to assert on JSON lines directly — same stance `logging-sync-crate` took).
