# Scope matrix test and attenuated tokens cannot widen (plan M6, plan §6)

Plan M6 acceptance: "Scope matrix test: for each (scope set × tool) pair, the expected allow/deny;
attenuated tokens can't widen." Design §6.2 defines the scopes and the macaroon model. This task is
the *test data and the attenuation properties*, not the token crypto (that is
[mcp-tokens](../mcp-tokens/notes.md)) nor the auth plumbing ([mcp-auth](../mcp-auth/notes.md)).

## Scopes (design §6.2, closed enum)

```rust
// crates/txtodo-mcp/src/token.rs
pub enum Scope {
    Read,                                   // list, search, resources, subscriptions
    Write(Write),                           // Write::{Add, Complete, Edit, Delete}
    Raw,                                    // line-level read/write, bypasses structure
}
pub enum Restrictor {
    Project(String),                        // project:+x
    Context(String),                        // context:@y
    File(FilePath),                         // file:work.txt
}
```

`write:delete` additionally requires `confirm: true` in the call. Restrictors intersect every base
scope: `write:add` + `project:+work` allows an add only if the task matches `+work`.

## The matrix is data, not prose

Encode every (tool × scope set) pair as a checked-in table with its expected allow/deny, and have
the test iterate the whole table.

```rust
// crates/txtodo-mcp/src/matrix.rs
pub struct Row { pub tool: Tool, pub scopes: &'static [Scope], pub restrictors: &'static [Restrictor],
                 pub allow: bool, pub confirm: bool /* required for write:delete */ }
pub const MATRIX: &[Row] = &[ /* … */ ];
```

Exhaustive, no default row (constitution §3): adding a tool or a scope must force a new row or the
iteration fails to cover it. The rows have to encode, at minimum:

- `write:delete` needs `confirm: true` — delete-without-confirm is deny regardless of scope.
- Restrictors intersect the base scope: `write:add` + `project:+work` allows an add only on a
  `+work` task; a `+other` add is deny even though the base scope allows it.
- `raw` bypasses structure and is its own row, not a superset of the rest.

## Attenuation widens nothing

A macaroon holder can mint a *narrower* token, never a broader one (design §6.2). Property to pin:

- **Narrowing is monotone.** `attenuate(token, caveat)` may only remove scope or tighten a
  restrictor; verify it never returns a `Scope` that is a strict superset of the parent's.
- **Attempted widenings all fail to mint** — read-only + `write:add`; `+work` + `+other`; expired +
  longer expiry — each rejected at `attenuate` time.
- **Tampering is not a widening path.** A token edited to drop a caveat fails macaroon verification
  (the chained HMAC breaks), so caveat deletion can never widen. macaroon crate:
  <https://docs.rs/macaroon>.

```rust
// crates/txtodo-mcp/src/token.rs
pub fn attenuate(token: &Token, caveat: &Caveat) -> Result<Token, AttenuateError>;
pub fn verify(token: &Token, root: &RootKey, revoked: &RevocationList) -> Result<Scope, VerifyError>;
```

## The concrete row set the tests drive

- root → `read` + `project:+work`: read `+work` allow, read `+other` deny, add `+work` deny,
  add `+other` deny.
- root → `write:add` + `context:@phone`: add `@phone` allow, add `@laptop` deny.
- root → `write:delete` without `confirm`: delete deny; with `confirm: true`: delete allow.
- root → `raw`: line-level read and write allow; structured `todo_add` deny.

## Placement/dependencies

- `txtodo-mcp` owns `Scope`/`Restrictor`/`matrix.rs`; `allowedDeps` = `txtodo-proto`, `txtodo-query`.
- No daemon state needed — the matrix is pure over `Scope × Tool`, so the test is a unit test in
  `txtodo-mcp`, not an integration test.

## Acceptance

- Iterating `MATRIX` asserts each (scope set × tool) pair matches its expected allow/deny.
- A new tool or scope fails the iteration until a row is added (no default).
- Each widening attempt fails to mint; the tampered token fails verification.

## References

- macaroon crate: https://docs.rs/macaroon
- design §6.2 scope table (this repo, `txtodo-design.md`)
