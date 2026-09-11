# Tests: a peer without the group key is rejected, a tampered op is rejected (plan M4 acceptance)

## Overlap, deliberately

[sync-crypto-envelope](../sync-crypto-envelope/notes.md) already lists unit tests for both
properties (wrong key / wrong group / wrong epoch; one flipped byte fails verification). This task
is **not** a second copy of those — it is the same two properties asserted against two real daemons,
where the rejection has to survive the session state machine, the import path and the store.

Keep the split explicit so neither set gets deleted as redundant later:

| level | where | proves |
|---|---|---|
| unit | `txtodo-sync` | the primitive rejects |
| integration | two `txtodod` processes | nothing downstream *accepts anyway* |

The integration case is the one that catches the real bug: a verify that returns `Err` which some
caller logs and continues past. CLAUDE.md §3 forbids log-and-continue; this is the test that proves
it.

## Assert on the log, not just the file

A rejected op must leave **no** trace: not in `ops`, not in the projection, not in the file. Assert
all three. A test that only checks `todo.txt` passes even if the op was stored and merely not
materialised — which would then sync onward to a third device, and that is the failure that
matters.

Also assert the *reason* reaches the operator: a rejection is a security event, so it must appear in
the daemon's JSON logs (`tasks/daemon-tracing-logs`) with the peer and the cause. Silent rejection
and silent acceptance are equally bad for debugging.

## Building a tampered op honestly

The temptation is to construct an invalid op in Rust and hand it to the import function. That tests
the function, not the daemon. Tamper at the wire: capture the frame the sending daemon produced,
flip one byte in the op payload, forward it. The `Link` trait
([sync-lan-transport](../sync-lan-transport/notes.md)) makes that a ten-line man-in-the-middle
implementation, and it is the same seam the MITM pairing test uses.

Three tamper positions, three distinct expected errors — they exercise different code:

- a byte inside the signed op payload → signature failure
- a byte in the AEAD ciphertext → tag failure, decryption never happens
- a byte in the cleartext header (`key_epoch`) → unknown or wrong epoch

## The unkeyed peer

Two shapes, both worth a test:

- A peer that knows the group **id** (it is in the mDNS TXT record, by design) but not the key. It
  must get past discovery and fail at the first sealed batch — the failure is expected to be late,
  and the test should assert it fails *at all*, not where.
- A peer from a different group entirely, dropped before a connection opens.

## Negative space

Assert what must never be true (CLAUDE.md §3): after any of these runs, the victim daemon's op count
is unchanged and its `todo.txt` is byte-identical to before. That single pair of assertions is
stronger than any number of error-variant checks, and it keeps passing when the error types are
refactored.
