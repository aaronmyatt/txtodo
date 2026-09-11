# Scope matrix test and attenuated tokens cannot widen (plan M6, plan §6)

Plan M6 acceptance: "Scope matrix test: for each (scope set × tool) pair, the expected allow/deny;
attenuated tokens can't widen." Design §6.2 defines the scopes and the macaroon model.

## The matrix is data, not prose

Scopes (§6.2): `read`, `write:add`, `write:complete`, `write:edit`, `write:delete` (needs
`confirm: true`), `raw`, and the restrictors `project:+x`, `context:@y`, `file:work.txt`.

Encode every (tool × scope) pair as a checked-in table with an expected allow/deny, and have the
test iterate the whole table. Exhaustive, no default row (CLAUDE.md §3): adding a tool or a scope
must force a new row or the matrix fails to cover it.

Rules the matrix has to encode:

- `write:delete` requires `confirm: true` in the call — delete-without-confirm is deny regardless.
- Restrictors intersect the base scope: `write:add` + `project:+work` allows an add only if the
  task matches `+work`; anything else is deny even though the base scope allows it.
- `raw` bypasses structure (line-level read/write) and is its own row, not a superset of the rest.

## Attenuation widens nothing

A macaroon holder can mint a narrower token, never a broader one. The test cases:

- root → `read` + `project:+work`: add on `+work` deny, add on `+other` deny, read `+other` deny,
  read `+work` allow.
- Attempted widenings all fail to mint: read-only + `write:add`; `+work` + `+other`; expired +
  longer expiry.
- Tampering: a token edited to drop a caveat fails macaroon verification (macaroon crate,
  <https://docs.rs/macaroon>), so caveat deletion is never a widening path.

## Bounds

Scope set is a small closed enum; the matrix table is a `const` slice. No unbounded input.
