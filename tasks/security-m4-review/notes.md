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
