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
