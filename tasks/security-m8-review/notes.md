# Security checklist review before M8 close (plan M8, plan §5 "Security checklist")

Plan §5, the M8 gate, same checklist M4 reviewed. [security-m4-review](../security-m4-review/notes.md)
decided the review's output is checked-in tests + a `RATCHET.md` findings section, not a person
re-reading code. M8 inherits M4's green tests and adds the carriers that did not exist then: relay,
file carrier, bundle.

## Item by item, at M8 scope

| item | M8 status | what changed since M4 |
|---|---|---|
| no secrets in logs | in scope | relay + file-carrier + bundle paths are new logging surfaces; re-run the tracing-capture test over them |
| keys only in keystore | in scope | the bundle wraps the group key under a passphrase — assert no key material appears in the bundle manifest clear text |
| every network message versioned, authenticated, encrypted | in scope | relay and file carrier carry the same encrypted frames; bundle manifest must be inside the passphrase wrap, not clear |
| MCP HTTP refuses non-loopback unless `--lan` | inherited green | unchanged; M4/M6 tests still pass |
| tokens never logged | inherited green | unchanged |
| path traversal impossible via `ref:` | inherited green | fuzzed at M4; but the file-carrier folder is **new** user input — symlink/`..` escape of `--sync-dir` must be checked |
| relay cannot distinguish op types | **now checkable** | was deferred from M4; [relay-reference](../relay-reference/notes.md) + [relay-converge-test](../relay-converge-test/notes.md) assert it |

## The three new carriers are the review

- **Relay:** store typed `Vec<u8>`, no structure; the converge test asserts the relay holds only
  opaque bytes. Check the relay's own logs leak nothing (it logs routing metadata, not payload).
- **File carrier:** `--sync-dir` is external input — validate it is a real dir, refuse symlink
  escape, never follow `..` outside it. Ops on disk are ciphertext; assert that.
- **Bundle:** the manifest (device id, schema, hashes) is fine in the clear; the group key is not.
  Assert no key bytes in the manifest, and that a wrong-passphrase import fails without writing.

## Deliverable

A dated `RATCHET.md` section listing each checklist item `pass` / `fail` / `deferred to M<n>` with
the test proving it. Deferred items get a todo.txt line in the owning milestone — the relay "cannot
distinguish op types" item closes here, do not carry it to M9 as a note nobody reads.
