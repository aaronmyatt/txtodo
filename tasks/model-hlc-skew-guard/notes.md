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
