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

## As built (2026-09-13, agent) — unit level only

The real blocker (flagged as `task_6fe73971`, and in `sync-crypto-envelope`'s own notes): `Session`
decided what was legal but nothing ever called `sign`/`verify_batch`/`seal`/`open`. Closed that gap
rather than writing tests against primitives that were already proven in isolation.

- `crates/txtodo-sync/src/message.rs`: `Message::Ops` gained `signatures: Vec<Signature>`, parallel
  to `ops`. `check_caps` now refuses a length mismatch as `MessageError::SignatureCount` before the
  range checks. This is the wire-shape change `sync-crypto-envelope`'s notes deferred ("redesigning
  `Message::Ops`'s wire shape... deserves its own focused pass") — that pass. Golden
  `goldens/ops.postcard` regenerated (`TXTODO_UPDATE_GOLDENS=1`); every other golden untouched.
- `crates/txtodo-sync/src/sealed_ops.rs` (new): the actual send/receive path. `seal_ops(ops, ranges,
  &DeviceSigningKey, &SealContext)` signs every op, builds `Message::Ops`, then seals the encoded
  body with the group key (`SealContext` bundles group/epoch/key — otherwise `seal_ops` broke the
  workspace's 5-argument cap). `open_ops(&Frame, GroupId, &GroupKeys, &BTreeMap<DeviceId,
  DevicePublicKey>)` opens first (a wrong/absent group key fails here, before a byte of `Message` is
  parsed), decodes, then runs `verify_batch` on the `Ops` variant. `SealedOpsError` wraps
  `CryptoError`/`MessageError` so a caller matches one type. A caller that gets `Ok` from `open_ops`
  hands the `Message` straight to `Session::on_ops`.
- `crates/txtodo-sync/src/session.rs`: `on_ops` takes `device_keys: &BTreeMap<DeviceId,
  DevicePublicKey>` and calls `verify_batch` first, before the existing "was this run requested"
  check — a batch with a bad signature or an unrecognised device never touches `wanted`/`inflight`.
  `SessionError::Crypto(CryptoError)` carries the reason. `Session` still never touches the group-key
  AEAD directly (`sealed_ops` owns that); it only ever sees an already-opened `Message`.
- Tests, all unit-level, no sockets, no second process (`crates/txtodo-sync/src/sealed_ops_tests.rs`
  and two new cases in `session_tests.rs`):
  - `a_peer_sealing_with_the_wrong_group_key_is_rejected` — a peer that knows the group id but seals
    with a different key: `open_ops` → `CryptoError::Decrypt`.
  - `a_peer_with_no_group_key_at_all_is_rejected` — an unpaired peer's `GroupKeys` is empty:
    `CryptoError::UnknownEpoch { held: 0 }`, not a decrypt attempt against nothing.
  - `a_frame_tampered_after_sealing_is_rejected_before_any_op_is_read` — one flipped byte in the
    sealed `Frame.body` (the wire-tamper shape from "Building a tampered op honestly" above, done
    in-process rather than through a `Link` MITM): `CryptoError::Decrypt`.
  - `session_on_ops_rejects_a_signed_batch_tampered_after_signing` — an op edited after signing,
    fed straight to `Session::on_ops` (no seal/open involved): `SessionError::Crypto(SignatureInvalid)`,
    and the session stays in `Wanting`, never `Importing`.
  - `session_on_ops_rejects_a_batch_from_a_device_with_no_known_key` — `SessionError::Crypto(UnknownDevice)`.
  - `a_genuine_sealed_batch_opens_and_flows_straight_into_session_on_ops` — positive control: the
    happy path still reaches `Importing` end to end.
  - `open_ops_surfaces_a_message_error_when_the_sealed_plaintext_is_not_a_valid_message` — a batch
    that decrypts cleanly but isn't a valid `Message` is a `MessageError`, not a `CryptoError`.
- `cargo test -p txtodo-sync`: 136 passed, 1 ignored (the pre-existing, documented iroh loopback
  blocker), 0 failed. `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D
  warnings`, `cargo fmt --all --check`, file-length and boundary checks all clean after this change
  (see the session's own report for exact counts).

**Not done here, still open (out of scope for this task):**
- The two-daemon integration variant from the table above (`unit` row done; `integration` row is
  not). Needs two real `txtodod` processes, the `Link`-level MITM described in "Building a tampered
  op honestly", and the daemon import path (`verify_batch`/`open_ops` called from the daemon, not
  just from `txtodo-sync` tests) — none of which exist yet. Also still open from that row: asserting
  the rejection reaches the daemon's JSON logs (`tasks/daemon-tracing-logs`) with peer and cause, and
  the "negative space" assertion (op count and `todo.txt` unchanged) at the store level.
- The three-distinct-tamper-position table (signed payload / AEAD ciphertext / cleartext header) is
  only partly reachable at unit level: because `seal_ops` seals the *entire* encoded `Ops` body in
  one AEAD blob, any wire-level byte flip fails the tag (`CryptoError::Decrypt`) before signature or
  header parsing ever runs — there is no wire-level way to reach "signature failure" or "wrong
  epoch" independently by flipping ciphertext bytes. The signature-failure case is covered instead by
  tampering the op *before* sealing/signing-mismatch (`session_on_ops_rejects_a_signed_batch_
  tampered_after_signing`); the "wrong/unknown epoch" case is covered by `open`'s own AAD/header
  parsing (`aead_tests.rs`, and `a_peer_with_no_group_key_at_all_is_rejected` here). Getting a true
  three-way split at the wire needs the integration test's real `Link` MITM forwarding a captured
  frame, per the task notes — that is part of the still-open integration half above.
