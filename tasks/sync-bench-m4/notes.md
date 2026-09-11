# Bench: 1 000 ops between loopback daemons ≤ 500 ms, idle RSS ≤ 50 MB (plan M4)

Two numbers, two completely different kinds of measurement. Do not put them in one harness because
they share a milestone.

## The throughput number belongs in criterion; the memory number does not

`tasks/daemon-reconcile-bench` already established the pattern: criterion benches with a budget
checked by `.claude/scripts/check-bench.sh`, the number living in `.claude/budgets.json`. Follow it
for the 1 000-op sync — same script, a second budget key.

Resident set size is not a criterion measurement. Criterion times a function many times in one
process; RSS is a property of a process at rest. It needs its own check: start `txtodod`, let it
settle, read RSS, compare. Platform-specific (`/proc/self/status` `VmRSS` on Linux,
`proc_pid_rusage` / `ps -o rss=` on macOS), so it must degrade to a skip on unsupported platforms
rather than fail CI on Windows.

## Both numbers need a definition before they mean anything

A budget that does not say what it measures cannot fail honestly.

- **1 000 ops ≤ 500 ms** — from when? Measure the **steady-state** transfer: both daemons already
  paired and connected, 1 000 ops queued on A, clock from first frame sent to B's log holding all
  1 000 **committed** (not received — `Ack` is defined on committed ranges in
  [sync-protocol-frames](../sync-protocol-frames/notes.md), so the bench should use the same
  definition). Discovery and pairing are excluded; they are the 2 s budget in
  [sync-loopback-converge](../sync-loopback-converge/notes.md), and mixing them makes both numbers
  meaningless.
- **Idle RSS ≤ 50 MB** — "idle" = daemon started, workspace walked, watcher armed, no sync peer, no
  edits for a bounded settle period. State the workspace size the number is measured against: RSS
  on an empty workspace and on a 10 k-line one are different claims, and the 10 k-line workspace is
  the one the project's other budgets use.

Write both definitions into `budgets.json.notes`, the way `latencyMs` and `payloadKB` already carry
their reasons.

## Loopback is not the network

A 500 ms loopback number says nothing about wifi. That is fine — it is a regression guard on **our**
code, not a performance claim — but say so in the bench's doc comment, or someone will quote it in
a README.

## Watch for the bench measuring the wrong thing

Two specific ways this bench can quietly become useless:

- **Batching hides per-op cost.** 1 000 ops in one `Ops` frame measures one frame. Run both shapes:
  one batch, and `MAX_OPS_PER_BATCH`-sized batches, and record both. The second is the realistic one.
- **Crypto excluded.** If the bench path skips the AEAD and signatures, it measures the fast half.
  Signatures at 1 000 ops are the plausible bottleneck, so they must be in the measured path.

## Deliverable

- Criterion bench + a `syncMs` budget key in `budgets.json`, wired into `check-bench.sh`.
- A separate `idleRssMb` check with its own script, skipping where unsupported.
- The measured numbers recorded in `./notes.md` at the time they first pass, so later drift has a
  baseline to be compared against rather than just a pass/fail.
