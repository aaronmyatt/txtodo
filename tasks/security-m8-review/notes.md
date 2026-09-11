# Security checklist review before M8 close (plan M8, plan §5)

## Goal

Plan §5's security checklist runs before M4, M6 and M8 close. M8's review covers the carriers that
did not exist at M4: relay, file carrier, bundle. The output is checked-in tests plus a dated
`RATCHET.md` findings section — not a person re-reading code (that decision is
[security-m4-review](../security-m4-review/notes.md)).

## Design

### The checklist, item by item, at M8 scope

| # | Item (plan §5) | M8 status | Proof |
|---|---|---|---|
| 1 | no secrets in logs | re-check | tracing-capture test re-run over relay/file-carrier/bundle log paths; no key/passphrase bytes in JSON logs |
| 2 | keys only in keystore | re-check | bundle wraps the group key under a passphrase; assert no key bytes in the manifest clear text |
| 3 | every network message versioned, authenticated, encrypted | re-check | relay + file carrier move the same frames (sync-crypto-envelope); bundle manifest is inside the passphrase wrap, not clear |
| 4 | MCP HTTP refuses non-loopback unless `--lan` | inherited green | M4/M6 tests unchanged |
| 5 | tokens never logged | inherited green | unchanged |
| 6 | path traversal impossible via `ref:` | re-check | ref slug fuzz (M4) unchanged; **new**: `--sync-dir` is external input — refuse symlink escape / `..` outside the chosen folder |
| 7 | relay cannot distinguish op types | closes here | relay-converge-test asserts opaque blobs; relay-reference logs routing metadata only |

Items 4/5 carry forward M4's green tests unchanged; items 1–3, 6 and 7 are re-proven against the
new surfaces. External input (`--sync-dir`) is VALIDATED, not asserted — constitution §3.

### The three new carriers are the review

- **Relay:** the store holds typed `Vec<u8>`, no structure. Assert the relay's own logs carry
  routing metadata, never payload. Close the M4-deferred op-type item.
- **File carrier:** `--sync-dir` is user input, so validate it: real directory, refuse symlink
  escape, never follow `..` outside it. Ops on disk are ciphertext only — assert.
- **Bundle:** the manifest (device id, schema version, hashes) is fine in the clear; the group key
  is not. Assert no key bytes in the manifest; assert wrong-passphrase import fails without writing.

## Placement/dependencies

- Deliverable is a dated section appended to `RATCHET.md` — **frozen path + append-only** (fence:
  ask), listing each item `pass`/`fail`/`deferred to M<n>` with its test. Never edit a prior line.
- Tests live beside the surfaces they prove: `crates/txtodo-daemon/tests/security_m8.rs` (log
  capture, `--sync-dir` escape), the relay/blob assertion in
  [relay-converge-test](../relay-converge-test/notes.md), and the bundle manifest test in
  [cli-bundle](../cli-bundle/notes.md).

## Edge cases & invariants

- Deferred items get a `todo.txt` line in the *owning milestone* — never a prose note nobody reads.
  Item 7 closes here; it must not be carried to M9 as an untracked sentence.
- A `fail` is a release blocker, not a "known issue": M8 does not close until every item is
  `pass` or `deferred` with a milestone line.
- "Inherited green" items are asserted by re-running their M4/M6 tests in the M8 CI job, not by
  asserting they "probably still pass".

## Acceptance

- `RATCHET.md` gains a dated M8 findings section: 7 rows, each `pass`/`fail`/`deferred to M<n>`,
  each naming the test that proves it.
- Relay logs leak no payload; relay store is opaque (item 7 closed).
- `--sync-dir` refuses symlink escape and `..` traversal (item 6 re-proven for the new input).
- Bundle manifest carries no key bytes; wrong-passphrase import writes nothing (items 1–3 re-proven).
- M4/M6 tests re-run green in the M8 job (items 4/5 inherited).

## References

- plan §5 (txtodo-implementation-plan.md), design §4.6 (txtodo-design.md)
- Sibling: [security-m4-review](../security-m4-review/notes.md), [relay-reference](../relay-reference/notes.md), [relay-converge-test](../relay-converge-test/notes.md), [cli-bundle](../cli-bundle/notes.md)
