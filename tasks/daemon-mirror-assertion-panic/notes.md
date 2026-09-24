# Mirror/state divergence panics a worker thread, daemon looks alive but isn't

## Reported

Found 2026-09-18 in `~/Library/Logs/txtodo/launchd.err.log`, from an old debug-build `txtodod`
(from a since-deleted worktree). Two panics, same assertion, different call sites:

```
thread 'tokio-rt-worker' (...) panicked at crates/txtodo-daemon/src/mirror_converge.rs:29:9:
converged
thread 'tokio-rt-worker' (...) panicked at crates/txtodo-daemon/src/actor_mirror.rs:126:9:
assertion failed: self.mirror.agrees_with(&self.state)
```

and, separately in the same log, `thread 'main' (...) panicked` at the identical
`actor_mirror.rs:126:9` assertion on two other occasions.

## Why it isn't already possible to trust "the daemon answers" as "the daemon is healthy"

`actor_mirror.rs` has three `debug_assert!(self.mirror.agrees_with(&self.state))` calls
(`flush_mirror` line 84, `converge_mirror` line 104, `resync_mirror` line 126) — each one right
after a code path *whose entire job* is to make the mirror agree with state (a flush, a
best-effort converge, or the last-resort full rebuild). `resync_mirror` panicking means even the
last-resort rebuild didn't produce an agreeing mirror — this isn't a rare timing fluke, it's the
final safety net failing.

Because `debug_assert!` only exists in debug builds, this fires in dev/test daemons but is silently
absent in release. And because the panic is on a `tokio-rt-worker` thread (or a request-driven
`'main'` call to a blocking RPC handler, per the other two log entries), Rust's default panic
behavior unwinds *that task*, not the process — `ps`/`launchctl` still show the daemon "alive",
and its unix socket still accepts new connections, but the document actor that panicked is now in
an undefined state (or gone). Nothing downstream currently distinguishes "healthy daemon" from
"daemon process alive but one of its document actors silently died."

## What to reuse

- `crates/txtodo-daemon/src/mirror.rs` — `Mirror::flush`/`converge_to`/`from_state`, and
  `agrees_with` itself; the CLAUDE.md doc for this crate already flags `mirror.rs`'s doc comment
  about a known, unrelated, super-linear idle-RSS cost in the same adopt/replay pipeline
  (`tests/idle_rss.rs`, `#[ignore]`d, ~1.7GB against a 50MB budget) — worth checking whether that
  same pipeline is implicated here too, or a genuinely separate bug.
- `crates/txtodo-daemon/src/actor_mirror.rs::flush_refused`/`converge_failed` — the existing
  escalation chain (flush failure → converge; converge failure → full resync) that this bug means
  is *still not always sufilcient*, since the escalation's own last step is what's asserting.
- The actor's own mailbox/task supervision (wherever `FileActor`'s tokio task is spawned and
  awaited) — needed for item 4 in `todo.txt` (noticing a panicked actor rather than silently
  losing it).

## Design notes

- Start from reproduction, not the fix: `debug_assert!` existing at all means someone intended
  this to be a true invariant, so the right first question is what op/edit sequence breaks it, not
  whether to keep or remove the assertion.
- Given the CLAUDE.md's own framing ("the mirror never decides bytes... a refusal is logged and
  healed by a rebuild, never a client error"), the design intent is clearly "this must never be
  visible to a client" — a debug-only crash is arguably a worse outcome than the thing it's
  guarding against, since it takes down more of the daemon's usefulness than a quietly-wrong
  mirror would. Any fix should preserve "never a client error" while turning "silently keep
  running with a possibly-wrong actor" into something a health check (or `txtodo doctor`) can
  actually see.

## Acceptance

Every checklist item in `todo.txt`, plus: in a release build, the same op sequence that reproduces
the divergence in debug either (a) no longer diverges (root cause fixed), or (b) is caught,
logged at `error`, and the affected document is refused/recovered instead of served from a
possibly-inconsistent actor.

## Not in scope

- The separate `cli-global-socket-cwd-fallback` and `daemon-stale-service-repair` tasks, found
  during the same investigation but independent bugs.
- The pre-existing, separately-tracked idle-RSS/super-linear-memory issue in the same mirror/adopt
  pipeline (`tests/idle_rss.rs`) — only worth cross-checking if the repro here turns out to share a
  root cause.

## As built (2026-09-18)

**Root cause, found by reading, then confirmed by a real reproduction**: `mirror_converge.rs`'s
`place()` stands a not-yet-real blank in for a `BlankInsert` it hasn't applied yet, using a single
shared `PLACEHOLDER` sentinel constant. The walk keeps its own local `visible: Vec<TaskId>`
simulation of what the mirror *will* look like once every corrective op lands. The moment two or
more pending placeholders coexist in that `Vec` (i.e. the state wants three or more brand-new
consecutive blank lines in one converge call), `position()`'s `.iter().position(|v| *v == id)`
(first-match) can no longer distinguish "the placeholder I just inserted" from "an earlier,
already-consumed one with the identical value" — the third+ new blank silently reads as
"already there" against the wrong slot, and its `BlankInsert` is never emitted. Reproduced exactly
(same file, line, and panic message as the real log:
`mirror_converge.rs:29:9: converged`) via
`mirror_tests::converge_inserts_three_consecutive_brand_new_blank_lines` before touching any
production code.

**Fix**: `place()` now mints a fresh, unique placeholder per pending blank
(`mint_placeholder`, counting down from `u128::MAX` — only ever touches the low 120 bits for any
realistic document, since real blank ids grow upward from a small counter, so the two ranges can't
collide). `apply_corrections` resolves each placeholder to the real sentinel *its own*
`BlankInsert` minted via a `HashMap<TaskId, TaskId>`, replacing the old single
`last_blank: Option<TaskId>` (which happened to work for the 2-blank case only because ops are
applied in the same order they're generated — fragile reasoning, not a real fix; the map is
correct regardless of how the algorithm's op ordering evolves later).

**Defense in depth, per this task's own item 3**: the three `debug_assert!`s in `actor_mirror.rs`
(`flush_mirror`, `converge_mirror`, `resync_mirror`) — compiled out entirely in a release build,
so a *different*, still-undiscovered divergence would have silently served a wrong mirror in
production — are now always-on checks. A disagreement after `flush` escalates to `converge`; a
disagreement after `converge` escalates to `resync` (the true last-resort rebuild, which does not
share `place()`'s placeholder machinery at all); a disagreement even after `resync` has nothing
left to escalate to and is logged at `error` (`mirror_resync_still_disagrees`) rather than trusted
silently. Verified this genuinely changes release-build behavior: `cargo test -p txtodo-daemon
--lib --release` (this repo's `Cargo.toml` has no `debug-assertions = true` override for the
release profile) still passes the reproduction test, proving the always-on checks — not a
compiled-out `debug_assert!` — are what's catching it now.

**Tests**: `mirror_tests.rs` gained
`converge_inserts_three_consecutive_brand_new_blank_lines` (the exact reported crash) and
`converge_extends_an_existing_blank_run_with_several_more` (a pre-existing blank plus four new
ones, a variant that only manifests once at least 4 total are wanted from a mirror already holding
one real blank). Both green in `--lib` and `--release`; full `cargo test -p txtodo-daemon --lib`
(223 tests) and `cargo clippy -p txtodo-daemon --lib -- -D warnings` both clean.

**Item 4 left open, `@human`**: a worker-thread panic anywhere in `FileActor`'s mailbox loop
(`actor.rs:133`'s `tokio::spawn(self.run(rx))` discards its `JoinHandle`) still silently ends that
one document's actor with nothing noticing, independent of the specific bug this task fixed. Real
supervision needs an actual design decision (where "this document is unavailable" state lives, how
`resolve()`/gRPC surfaces it, auto-restart vs. refuse-until-reopened) that's a
`workspace_catalog.rs`/`actor.rs` architecture change, not a mirror bugfix — flagged, not decided
here.

## Decision: actor panic (2026-09-24, human)

Restart plus a log line. Today a panic already turns later requests into `ActorError::Gone` →
gRPC `unavailable` (never a stale answer); what was missing is noticing it. So: keep the
`JoinHandle`, log the panic at error with the document path, and respawn the actor from disk.
Restarts are bounded so a panic on every load can't loop; past the cap the document stays Gone
(today's behaviour) with one log line. Rejected: refuse-until-reopened (a user would have to know
to reopen). Not decided here: whether Health/doctor shows a restart count — add it if a restart
ever needs diagnosing without the logs.
