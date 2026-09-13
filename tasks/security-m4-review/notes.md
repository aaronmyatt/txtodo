# Security checklist review before M4 close (plan §  "Security checklist")

The checklist is one line of prose in the plan (M4/M6/M8 gate):

> no secrets in logs; keys only in keystore; every network message versioned, authenticated,
> encrypted; MCP HTTP refuses non-loopback unless `--lan`; tokens never logged; path traversal
> impossible via `ref:` (fuzz the slug validator); relay cannot distinguish op types.

## A review nobody can re-run is a signature, not a review

Four of those seven items are mechanically checkable, and the ones that are should be **tests**, not
a person reading code once per milestone. Otherwise M6's review re-derives M4's conclusions from
scratch and finds the same things, or worse, assumes they still hold.

So this task's output is two things:

1. `RATCHET.md`-recorded findings for anything wrong (it owns exceptions and debt, per CLAUDE.md §8).
2. A checked-in test per mechanisable item, so M6 and M8 inherit them green instead of re-reading.

## Item by item, at M4 scope

| item | M4 status | how it is checked |
|---|---|---|
| no secrets in logs | in scope | test: run a sync + pair with `tracing` capture, assert no key/passphrase bytes appear in the JSON logs |
| keys only in keystore | in scope | [sync-keystore](../sync-keystore/notes.md) already forbids argv/env; add a grep-style test for key material written outside the store |
| every network message versioned, authenticated, encrypted | in scope | [sync-protocol-frames](../sync-protocol-frames/notes.md) + [sync-crypto-envelope](../sync-crypto-envelope/notes.md); the gap to check is whether **every** variant is covered, including `Hello`, which is the tempting one to leave in the clear |
| MCP HTTP refuses non-loopback unless `--lan` | **M6** | not yet buildable; note it and move on |
| tokens never logged | **M6** | same |
| path traversal impossible via `ref:` (fuzz the slug validator) | **M5** builds refs, but the validator is **M1 core** and exists now | fuzz it now — see below |
| relay cannot distinguish op types | **M8** | note it; the M4-relevant half is that `Ops` frames should not leak type via length, which is worth recording as a known gap now rather than discovering at M8 |

## `Hello` is the item most likely to be wrong

`Hello` carries the group id and heads and is sent **before** a shared key is in use for that
session. It is the natural place to have left something unauthenticated. Check specifically: is
`Hello` covered by the version envelope, and does an attacker who replays a captured `Hello` learn
or change anything? Heads are a privacy leak (how much you have written) even if not a correctness
one.

## Fuzz the slug validator now, not at M5

`core::…::ref_slug` validation shipped at M1 (`tasks/core-task-views`), the fuzz harness exists
(`tasks/core-fuzz-targets`, `fuzz/`), and M5 will build real directory paths from those slugs. Path
traversal is cheapest to rule out before anything joins a slug onto a path. Add a `fuzz_target` that
asserts a validated slug never contains `/`, `\`, `..`, a NUL, a leading dot, or a Windows reserved
name, and run it for a bounded session.

## Deliverable

A dated section in `RATCHET.md` under **Exceptions granted** / findings, listing each item as
`pass` / `fail` / `deferred to M<n>`, with the test that proves it or the reason it cannot be tested
yet. Deferred items get a todo.txt line in the owning milestone, not a note that nobody reads.

## As built (2026-09-13, agent)

Full item-by-item verdicts, test references and findings are in `RATCHET.md`'s 2026-09-13
`security-m4-review` entry — not duplicated here. Summary:

- **No secrets in logs** — pass. `crates/txtodo-daemon/src/lan_session_security_tests.rs` (new
  file; the existing `lan_session_tests.rs` had this session's other test added to it first, then
  split out for the file-length budget — its scripted-peer helpers and `pairing_grpc_tests.rs`'s
  handshake/finalize helpers are now `pub(crate)` so both files could reuse them).
- **Keys only in keystore** — pass. `crates/txtodo-daemon/tests/security_keys_only_in_keystore.rs`
  (new file). A static source grep, not a runtime argv/env inspection: this sandbox's host has no
  portable `/proc/<pid>/cmdline` equivalent, and the task notes offer a static grep as an explicit
  alternative. Scope is deliberately narrowed to the workspace's actual argv/config surface (six
  named files) after a whole-workspace field scan produced three false positives on ordinary,
  unrelated identifiers that happen to contain "key" (a SQLite meta row key, a rotation epoch
  counter, the keystore's own internal passphrase field) — recorded in the test's own doc comment
  so a future reader does not have to re-derive why the scope is what it is.
- **Every network message versioned, authenticated, encrypted, including `Hello`** — pass, already
  built, not re-derived. `crates/txtodo-sync/src/hello_wire_tests.rs` (new file, six tests) proves
  it rather than re-deriving it: `Hello` is sealed by the same one production send path every other
  message uses (`txtodo-daemon`'s `lan_session.rs::send_message`), so it is never sent unauthenticated
  or before a shared key exists. The one open item is `@human`-tagged (Hello's heads leaking usage
  volume *within* the group) and is left open in `./todo.txt` below for a human decision.
- **Path traversal impossible via `ref:`** — fail, a real gap. New fuzz target
  `crates/txtodo-core/fuzz/fuzz_targets/slug_windows_safe.rs`, run 30 s locally (installed nightly +
  `cargo-fuzz` for this pass — neither is present by default). It crashed immediately:
  `is_valid_slug("con")` is `true`, a Windows-reserved device name the validator never checks for.
  `txtodo-core` is frozen this session; the fix is a follow-up, not attempted here. Getting the fuzz
  target to build at all needed one infrastructure fix: `crates/txtodo-core/fuzz/Cargo.toml` gained
  an empty `[workspace]` table (cargo-fuzz's own usual convention) because this worktree, being
  nested under the main checkout's own directory tree, made Cargo's git-boundary-based workspace
  discovery mistake the main checkout's `Cargo.toml` for the applicable workspace and refuse to
  build a package that manifest does not list as a member — unrelated to any code in this repo, and
  it changes nothing about what `cargo build/test/clippy --workspace` sees (this crate was already
  excluded from the real workspace).
- **MCP HTTP refuses non-loopback unless `--lan`**, **tokens never logged** — deferred to M6, per
  this task's own scope (not buildable: no MCP HTTP surface exists before M6).
- **Relay cannot distinguish op types** — deferred to M8 (no relay exists), with the M4-relevant
  half (sealed-frame length varies by message type, a potential size side-channel for a future
  relay) recorded in `RATCHET.md` now rather than discovered at M8.

Gates run this pass: `cargo build --workspace`, `cargo test --workspace` (new tests included, none
`--ignored`; the new `security_keys_only_in_keystore.rs`/`hello_wire_tests.rs`/
`lan_session_security_tests.rs` all run in the normal suite), `cargo clippy --workspace --all-targets
-- -D warnings`, `cargo fmt --all --check`, `.claude/scripts/check-file-length.sh`,
`.claude/scripts/check-boundaries.sh` — all clean. One pre-existing flake noted, not caused by this
pass: `tests/lan_loopback_converge.rs`'s real two-daemon mDNS convergence test missed its 2 s budget
once under this session's concurrent load, passed immediately when re-run alone.

Not done by this session, per the parent task's own instruction that editing the root `todo.txt` is
the orchestrating session's job: adding the M6/M8 todo lines item 10 above calls for, and completing
this task's parent line in the root `todo.txt`. Both are named explicitly in the final report instead.
