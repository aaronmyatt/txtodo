# Two daemons on loopback pair and converge within 2 s (plan M4 acceptance)

The end-to-end proof that M4 works: two **real** `txtodod` processes, different dirs, mDNS on
loopback, pair, edit, converge. Everything below it is unit-tested elsewhere; this is the one test
that fails when the pieces are individually correct and jointly wrong.

## This will be the fourth copy of the daemon spawn harness

`ABSTRACTIONS.md` (2026-09-12) already records three copies of "spawn `txtodod --dir <tmp>`, poll
for the socket, kill on drop": `txtodo-cli/tests/daemon_mode.rs`,
`txtodo-daemon/tests/support/mod.rs`, `txtodo-daemon/tests/crash.rs`. A two-daemon test needs it
twice more.

Do **not** extract it as a side effect of this task (CLAUDE.md §2, "extraction as a side effect is
forbidden"). Reuse `txtodo-daemon/tests/support/mod.rs` if this test lives in that slice — that is
reuse within a slice, which is just a function call — and append a line to the ledger noting the
count has grown. The human decides whether it graduates.

## The 2 s number

Plan M4: "converge within 2 s (measured in test)". That budget covers mDNS discovery + handshake +
one op round trip. Discovery dominates and is the flaky part.

- The clock starts **after both daemons report ready**, not at spawn. Process startup is not what
  the 2 s is about, and including it makes the test a CI-machine-speed test.
- Assert with a deadline, not a sleep: poll for convergence with a bounded loop and fail at the
  deadline. Never `sleep(2s); assert!(converged)` — that turns a fast pass into a slow one and
  hides regressions inside the margin.
- Record the *measured* time on success in the test output, so a drift from 200 ms to 1.9 s is
  visible before it becomes a failure.

## Loopback mDNS is the fragile part

mDNS on a CI runner is a real risk: no multicast, a container without it, or a runner where two
tests discover *each other's* daemons in parallel.

- Isolate by group id: every test run uses a fresh random group, and discovery drops other groups
  before connecting ([sync-lan-transport](../sync-lan-transport/notes.md)). That also stops a
  developer's own daemon on their laptop joining the test.
- If multicast is unavailable, the test must **skip with a clear message**, not fail and not
  silently pass. A silently-passing network test is worse than none; make the skip visible in CI
  output and count them.
- Keep a second variant that bypasses discovery and dials a known address, so the sync path is still
  covered where multicast is not. Discovery and convergence are two failures and should be two
  tests.

## What convergence means here

Both workspaces' `todo.txt` byte-identical, and both daemons' op logs holding the same op set. Bytes
alone would pass if one side never recorded the op and merely copied the file.

## Steps

1. Spawn A and B on temp dirs, wait for ready.
2. Pair them (scripted SAS confirmation through a test seam, not a TTY prompt — the seam is
   test-only and guarded).
3. Edit `todo.txt` in A's workspace externally, as a real file write.
4. Start the clock; poll B until byte-identical; assert under the deadline.
5. Reverse direction in the same test — a one-way test passes with a half-built protocol.
