# HLC with a clock-skew guard: warn or reject over 5 min drift (plan M4)

`crates/txtodo-model/src/hlc.rs` already has the **send** rule: `Hlc::tick(now_ms)` is strictly
monotone, counts within a millisecond, and survives a wall clock going backwards. Its own header
says the receive rule and the skew guard are M4. So this task adds exactly two things: `merge` and
the guard. Ref: Kulkarni et al., *Logical Physical Clocks*
<https://cse.buffalo.edu/tech-reports/2014-04.pdf> §3.

## The receive rule

```
fn merge(&mut self, remote: Hlc, now_ms: u64) -> Result<Hlc, HlcError>
```

```
w = max(self.wall_ms, remote.wall_ms, now_ms)
counter = match (w == self.wall_ms, w == remote.wall_ms) {
    (true,  true)  => max(self.counter, remote.counter).checked_add(1)?,
    (true,  false) => self.counter.checked_add(1)?,
    (false, true)  => remote.counter.checked_add(1)?,
    (false, false) => 0,   // the wall moved us forward on its own
}
```

Postconditions to assert: the result is `> self` **and** `> remote`, and `device` is still ours.
That pair is the whole point of an HLC and is cheap to check on every call.

## The guard — the decision worth making

Skew is not symmetric, and that asymmetry should drive the rule:

- A peer whose clock is **ahead** is dangerous. `wall_ms` only ratchets up, so merging one op from a
  device a year in the future pins our clock a year forward *permanently*, on every device that
  later syncs with us. There is no recovery short of editing the op log.
- A peer whose clock is **behind** is harmless to us. Its ops sort early; nothing we hold moves.

So: **reject ahead, warn behind.**

```rust
/// Largest peer clock lead we will merge. Beyond this the peer's clock, not ours, is wrong.
pub const MAX_PEER_SKEW_AHEAD_MS: u64 = 5 * 60 * 1_000;
/// Lag past which we warn the human; merging is still safe.
pub const MAX_PEER_SKEW_BEHIND_MS: u64 = 5 * 60 * 1_000;
```

## Reject *what*, exactly — for the human

"Reject" must not mean "drop the op", which is silent data loss. Two readings:

- **A — refuse the peer.** The skew check runs once per sync session, on `Hello` (todo
  `Sync protocol Hello Want Ops Ack`). A peer past the bound never gets to send ops; the session
  ends with a typed error naming both clocks, and `txtodo doctor` reports it.
- **B — accept the ops, clamp the clock.** Store them, but do not let `remote.wall_ms` advance ours;
  mark the batch for review.

Take **A**. The failure is a misconfigured machine, not a conflict, and it should be reported to the
human once rather than smeared across every op. B also breaks the `merge` postcondition
(`result > remote`), which is the invariant everything else leans on. `merge` itself still returns
`Err` on an out-of-bounds stamp so the model is safe even if a caller skips the `Hello` check —
belt and braces, one rule, two places it is enforced.

## Where it is wired

- `HlcError` grows a variant, so it stops being a unit struct: `Overflow { wall_ms }` and
  `PeerAhead { peer_ms, local_ms }`. Every `match` on it is exhaustive; the `Display` text must name
  both clocks and the bound, per CLAUDE.md §3 ("what was attempted and with which values").
- `txtodo doctor`'s existing `clock` check (`tasks/cli-doctor`) gains a line per known peer.
- The guard needs the wall clock, which is injected — `txtodo-daemon`'s `Clock` trait
  (`clock.rs:9`). `merge` takes `now_ms` as a parameter and stays pure; no clock reaches
  `txtodo-model`.

## Tests

Deterministic, no sleeps (CLAUDE.md §7): `now_ms` is a parameter, so every case is a table row.
Property worth having: for any pair of stamps and any `now_ms` within bounds, `merge` is
commutative in effect — merging `a` then `b` and `b` then `a` both yield a stamp greater than all
three inputs. That is the invariant, not literal equality.

## Reading taken (2026-09-12, agent, pending human confirmation)

Built under **A — refuse the peer**: `Hlc::merge` returns `Err(HlcError::PeerAhead { peer_ms,
local_ms, bound_ms })` and leaves the clock untouched; no op is dropped at the model layer because
the caller (the `Hello` handshake, later) never lets such a peer send ops. `Skew::check` is the one
shared rule for `merge`, `Hello` and `doctor`. If the human prefers **B — clamp**, `merge` loses
the `Err` branch and the `result > remote` postcondition, and the tests in `hlc_tests.rs` change
with it. The `txtodo doctor` per-peer line (subtask 7, @cli) waits until peers exist (`sync-pairing`).

## As built (2026-09-13, agent) — the `txtodo doctor` per-peer line

Peers now exist (`tasks/sync-device-remove`'s `devices` table, landed the same day), so this was
the remaining subtask. `Hlc::merge`/`Skew::check` themselves are untouched — this slice only wires
the already-built, already-tested rule through to a human-visible line.

- `crates/txtodo-proto`'s `Device` message carries a `SkewStatus` (`Unknown`/`Ok`/`Behind`/`Ahead`)
  and a `skew_ms` magnitude. `crates/txtodo-daemon/src/devices_grpc.rs` computes it server-side,
  once, via `txtodo_model::Skew::check(peer_ms, now_ms)` against each device row's
  `last_known_wall_ms` (`None` → `Unknown`, never a guessed verdict) — the same classification
  `txtodo device list` and `txtodo doctor` both read, so there is exactly one place this runs.
  `txtodo-cli` cannot depend on `txtodo-model` (constitution §2), which is why the enum crosses the
  wire pre-computed rather than the CLI re-deriving it from raw clock values.
- `crates/txtodo-cli/src/commands/doctor.rs` gained a sixth fixed check (`keystore`, plan M4
  `sync-keystore`, unrelated to this task but landed in the same pass) and a variable-length tail:
  one `peer` row per known, active (non-removed, non-self) device — `Ok`/`Behind` is a warn (safe
  to sync with), `Ahead` fails the whole command (a real sync session with that peer would be
  refused), `Unknown` warns rather than asserting a clock is fine when no sample exists yet.
  `debug_assert_eq!(checks.len(), 5, ...)` (five fixed checks) became `6` fixed checks plus
  `peer_checks(&devices)` appended after.
- **Known, honestly-scoped gap**: `last_known_wall_ms` is populated by exactly one code path today
  — a joiner registering the initiator's device at pairing — and that path does not yet have a
  genuine peer clock sample to put there (no wire message in the current local-RPC pairing surface
  carries one; see `tasks/sync-device-remove`'s own "As built" for why only one direction of
  registration exists at all yet). So every peer row a real user sees today will read `Unknown`
  until a live sync session (or a richer pairing exchange) actually observes a peer's wall clock —
  the column and the classification are real and tested, the data feeding them in production is
  not there yet. This is deliberately not papered over with a fabricated sample.
- Tests: `crates/txtodo-cli/tests/daemon_mode.rs::doctor_reports_the_keystore_backend_and_no_peer_rows_when_unpaired`
  asserts zero peer rows before any pairing. `crates/txtodo-daemon/src/devices_grpc.rs`'s own
  `tests` module unit-tests `skew_of` directly against `txtodo_model`'s own bounds
  (`MAX_PEER_SKEW_AHEAD_MS`/`MAX_PEER_SKEW_BEHIND_MS`): no sample is `Unknown`, and the
  Ok/Behind/Ahead boundaries and reported magnitudes match `Skew::check` exactly — since `skew_of`
  is a pure function, this needed no daemon, no store row, no fixture disconnected from a real run.
  `Skew::check`'s own exhaustive coverage is `crates/txtodo-model/src/hlc_tests.rs`, unchanged by
  this pass. What is *not* tested end to end is a real peer row actually reaching `Behind`/`Ahead`
  through a live daemon, because (per the gap above) nothing in this repo populates
  `last_known_wall_ms` from a real peer clock yet.
- `cargo build --workspace`, `cargo test -p txtodo-daemon -p txtodo-cli`, `cargo clippy --workspace
  --all-targets -D warnings`, `cargo fmt --all --check` all clean.
