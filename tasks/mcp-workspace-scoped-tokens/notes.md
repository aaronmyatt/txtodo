# mcp-workspace-scoped-tokens

## Goal
Design §6.2's capability tokens have a closed-union scope grammar: `read`, `write:*`, `raw`, and
three restrictor prefixes (`project:`/`context:`/`file:`) that narrow every other scope to
matching tasks *inside whichever one workspace the daemon/MCP happens to be scoped to*. Task
`mcp-multi-workspace-gateway` (just completed) made one daemon/MCP surface able to serve every
registered workspace via a `workspace: Option<String>` selector on every call. Today's token
grammar has no way to say which workspace(s) a token may reach at all — a token that can `write:add`
can do so in whichever single workspace happened to be open, and now that "whichever" can mean any
of several, that gap is real. This task extends the scope grammar with a fourth restrictor,
`workspace:`, so a token can be scoped to one workspace, a named set, or explicitly all of them.

## Design

### Grammar: `workspace:<id-or-path>` / `workspace:*`
Added to `RESTRICTOR_PREFIXES` in `crates/txtodo-daemon/src/tokens.rs` — now `["project:",
"context:", "file:", "workspace:"]`. Like the other three, the suffix is validated for shape only
(non-empty) — never resolved against the registry at create time, the same way `project:+x` is
never checked against an actual project.

The suffix's *value* follows the exact id-or-path convention `mcp-multi-workspace-gateway`
established for the wire `WorkspaceSelector` and `crates/txtodo-mcp/src/grpc_convert.rs`'s
`workspace_selector`:
- a `WorkspaceId` ULID (26-char Crockford base32, as returned by `WorkspaceList`), or
- a filesystem path.

Reusing that exact convention (rather than inventing a third shape) means a human copying an id out
of `todo_list_workspaces`/`WorkspaceList` into a `token create --scope workspace:<id>` call is doing
the same kind of copy-paste `mcp-multi-workspace-gateway` already trained them to do for every other
`workspace` argument.

### A set: repeat the restrictor, no new list syntax
`workspace:<id1>`, `workspace:<id2>` — two scope strings, both restrictors of the same kind. No
comma-separated value, no new delimiter. This was chosen over a literal `workspace:id1,id2` because:
- `scopes` is already `repeated string` on the wire and `Vec<String>` in storage — a list of
  restrictors is the data structure the grammar already has, not a new one.
- Every existing restrictor prefix's validation is a single non-empty-suffix check with no internal
  structure; adding comma-parsing to exactly one restrictor (and not `project:`/`context:`/`file:`)
  would make the grammar's four restrictors inconsistent with each other for no real gain — a `,`
  inside a project or context name is legal today (nothing forbids it) and would become ambiguous
  the moment one restrictor started splitting on commas and the others didn't.
- It costs zero new parsing code: `is_valid_scope`/`first_invalid_scope` already iterate every scope
  string in the list independently, so "one, a set, or all" for `workspace:` (and, incidentally, for
  every other restrictor too, if a future task ever wants that) falls out of the existing loop with
  no new branch.

### "All workspaces": explicit `workspace:*`
The literal suffix `*` — itself just a non-empty suffix, so no special-cased parsing was needed to
accept it; `is_valid_scope` treats it exactly like any other `workspace:<suffix>`.

Chosen over relying **only** on restrictor-absence to mean "all", even though absence already means
that (see Backward compatibility below), because:
- A token's scope list is read by humans (`TokenList`, `txtodo blame`, code review of a token-minting
  script) who benefit from an explicit, self-documenting marker that "every workspace" was a
  deliberate choice, not simply a token minted before this grammar existed or a forgotten restrictor.
- It is symmetric with the set form: `workspace:a`, `workspace:a,workspace:b`, `workspace:*` are all
  the same shape (one or more `workspace:` scope strings), rather than "all" being a structurally
  different case (bare absence) from "one" or "a set" (present, non-empty suffix).
- It costs nothing extra to validate — `*` is accepted by the exact same non-empty-suffix rule as
  every other value, no dedicated branch.

`workspace:*` and omitting `workspace:` entirely are equivalent in effect (unrestricted); they are
not deduplicated or normalized at create time — both are stored verbatim, same as every other scope
string (`create_token` stores `new.scopes` as given; see `crates/txtodo-store/src/tokens.rs`'s own
doc: "the daemon's gRPC boundary is where the design §6.2 closed union is enforced, once, at parse
time").

### Backward compatibility
Omitting `workspace:` entirely keeps meaning exactly what it always meant for `project:`/`context:`/
`file:`: unrestricted on that axis. A token minted before this task (no `workspace:` scope at all)
is unaffected — this generalizes design §6.2's original single-workspace-everything behavior to
"every workspace" now that one daemon/MCP surface can span many, the same generalization
`mcp-multi-workspace-gateway`'s own "omitted `workspace` selector resolves to the sole open
workspace" convention makes for a single *call* rather than a token's whole *grant*. No existing
token, no existing `TokenCreate` caller, and no existing test needs any change.

### How far this task goes: grammar + storage + creation-time validation only
Confirmed before writing any code (`crates/txtodo-daemon/src/tokens.rs`'s own doc comment, this
crate's `CLAUDE.md` Invariants, and `crates/txtodo-store/src/tokens.rs`'s own doc) that
**no request-time enforcement of any restrictor exists yet, for any scope** — `read`, `write:*`,
`project:`, `context:`, `file:` are all validated only at `TokenCreate` time and stored verbatim;
nothing on the wire calls `Store::verify_token`, and nothing anywhere checks an incoming RPC's
caller against a token's scopes before performing the operation. That is explicitly the larger,
separate, not-yet-built "MCP-auth-server milestone" (this crate's own `CLAUDE.md`: "Request-time
enforcement of a revoked/expired bearer is plan M6's larger MCP-auth-server milestone — out of
scope here; `Store::verify_token` is the primitive it will call").

Given that, this task does **not** build request-time workspace-restriction enforcement — doing so
for `workspace:` alone, while `project:`/`context:`/`file:` remain wholly unenforced, would be
inconsistent scope creep the backlog line didn't ask for and the codebase doesn't otherwise have
infrastructure for (no auth middleware, no per-call scope check of any kind exists to hook a new
restrictor into). What *is* in scope, and what shipped:
- Grammar: `workspace:` accepted as a fourth restrictor prefix, closed-union validated.
- Storage: no schema change needed — `scopes` was already `Vec<String>`/`repeated string`; a new
  restrictor prefix is just a new accepted string shape, not a new column or wire field.
- Creation-time validation: `an unrecognized scope is refused at create time, never silently
  accepted` extended to cover `workspace:` the same way it already covers the other three.

## Placement
All changes are inside `crates/txtodo-daemon/`:
- `src/tokens.rs`: `RESTRICTOR_PREFIXES` grown to 4, module doc comment extended, `#[cfg(test)]
  mod tests` added (none existed before) covering the new grammar directly against
  `is_valid_scope`/`first_invalid_scope`.
- `tests/tokens.rs`: two new integration tests over a real socket (workspace set + explicit-all
  round-trip; bare `workspace:` refused at create time), alongside the two pre-existing tests
  (`create_list_and_revoke_round_trip_over_the_socket`,
  `an_unrecognized_scope_is_refused_at_create_time`), both unmodified.
- `CLAUDE.md`: Invariants bullet on token scopes extended to name the fourth restrictor and its
  grammar, and to flag (again, explicitly) that it is not yet request-time enforced.

`crates/txtodo-cli/` was checked, not changed: this crate has **no** `Token` subcommand today
(`crates/txtodo-cli/src/cli.rs`'s `Command` enum has no `Token` variant at all — design §6.2's
`txtodo token create ...` example is aspirational, not yet implemented anywhere in this repo). The
only "token" hits in this crate are `mcp.rs`'s `--token <bearer>` flag (an opaque string passed
straight to the MCP client, no scope-string parsing) and `json.rs`'s unrelated `tokenize` lexer
function. Nothing here needed updating for this task.

No proto/wire change: `TokenCreateRequest.scopes`/`Token.scopes` were already `repeated string`
before this task; a new restrictor prefix is a client-side/daemon-side grammar convention layered
on an unchanged wire shape.

## Edge cases
- **`workspace:` with an empty suffix** (bare `workspace:`, no id/path/`*`): refused at create time,
  `InvalidArgument`, same as bare `project:`/`context:`/`file:` today — covered by
  `a_bare_workspace_restrictor_is_refused_at_create_time`.
- **`workspace:*` alongside a specific `workspace:<id>` in the same token**: both are individually
  valid scope strings, so both are accepted — grammar-wise this is redundant (an "all" restrictor
  and a specific one both present), not contradictory; no dedup/conflict logic was added because no
  enforcement exists yet to make the redundancy observable, and inventing conflict-detection for a
  restriction that isn't enforced would be speculative.
- **A `workspace:` value that isn't a real registered workspace's id/path**: accepted at create time,
  same as `project:+nonexistent-project` is today — this restrictor describes future scope, not a
  currently-resolvable reference, and the daemon's registry is not consulted at `TokenCreate` time
  for any restrictor.
- **Duplicate identical `workspace:<id>` entries** (`workspace:a`, `workspace:a`): both individually
  valid, both stored — no dedup, same as the pre-existing three restrictors would do if repeated
  (untested before this task, but the same code path).
- **Case/format of the id half**: not validated as a real ULID (no decode, only "non-empty suffix"),
  deliberately — matches every other restrictor's shape-only validation, and this crate *can*
  depend on `txtodo_model`/`Ulid` (it already does, for `TokenId`) but choosing not to decode keeps
  `workspace:` exactly as permissive as `project:`/`context:`/`file:` rather than singling it out
  for stricter treatment with no enforcement behind it to justify the asymmetry.

## Acceptance
- `workspace:<ulid>`, `workspace:<path>`, `workspace:*` are each individually valid scope strings,
  accepted at `TokenCreate`.
- A set (repeated `workspace:` entries) is accepted.
- A bare `workspace:` (empty suffix) is refused at `TokenCreate`, `InvalidArgument`, and stores
  nothing — matching the closed-union "refuse unrecognized, never silently accept" invariant.
- Every pre-existing token-scope test (`create_list_and_revoke_round_trip_over_the_socket`,
  `an_unrecognized_scope_is_refused_at_create_time`) still passes unmodified.
- Request-time enforcement is explicitly out of scope (see above) — not built, not claimed as built.

## As built (2026-09-16)

One commit (`crates/txtodo-daemon/` only), green (`cargo fmt -p txtodo-daemon`,
`cargo build -p txtodo-daemon`, `cargo test -p txtodo-daemon --lib --tests`) before landing:
- `src/tokens.rs`: `RESTRICTOR_PREFIXES` → `["project:", "context:", "file:", "workspace:"]`, module
  doc comment extended with the full grammar/reasoning, `#[cfg(test)] mod tests` added (4 tests):
  `a_bare_project_context_file_or_workspace_prefix_is_refused`,
  `workspace_restrictor_accepts_an_id_a_path_or_the_explicit_all`,
  `a_workspace_set_is_expressed_by_repeating_the_restrictor`,
  `an_unrecognized_scope_is_never_silently_accepted`.
- `tests/tokens.rs`: 2 new integration tests over a real socket:
  `a_workspace_restrictor_round_trips_including_a_set_and_the_explicit_all` (create with a two-entry
  workspace set, and separately with `workspace:*`, assert the scopes round-trip verbatim through
  `TokenCreate`'s response) and `a_bare_workspace_restrictor_is_refused_at_create_time` (mirrors the
  pre-existing `an_unrecognized_scope_is_refused_at_create_time` shape for the new restrictor
  specifically). The 2 pre-existing tests in this file are untouched and still pass.
- `CLAUDE.md`: Invariants bullet on token scopes extended in place.

### Deviations from the plan
None — the grammar shipped exactly as designed above: `workspace:` as a fourth restrictor prefix,
set-by-repetition, explicit `workspace:*` for all, absence-means-unrestricted for backward
compatibility.

### What proves it
- `cargo test -p txtodo-daemon --lib tokens::`: 4/4 new unit tests green.
- `cargo test -p txtodo-daemon --test tokens`: 4/4 green (2 new + 2 pre-existing, unmodified).
- `cargo test -p txtodo-daemon --lib --tests`: full crate suite green, no failures (11 test
  binaries' worth of `test result: ok`, 0 `FAILED`).
- `cargo build -p txtodo-daemon`: clean.
- `cargo fmt -p txtodo-daemon`: clean (applied once, re-verified).
- `cargo clippy -p txtodo-daemon --lib --tests -- -D warnings`: **cannot fully pass** — but not
  because of anything this task touched. `crates/txtodo-store/src/heads.rs` (3 functions) and
  `crates/txtodo-store/src/projections.rs` (2 functions) exceed the workspace's
  `cognitive-complexity-threshold = 10` (`clippy.toml`). This is pre-existing on a clean `main`
  checkout with none of this task's changes applied — verified directly: `git stash` (reverting
  every change this task made), `cargo clean -p txtodo-store`, then
  `cargo clippy -p txtodo-daemon --lib -- -D warnings` still fails with the identical 5 errors at
  the identical locations, before `git stash pop` restored this task's diff. `git diff --stat` for
  the whole session never touched `crates/txtodo-store/` at all. `mcp-multi-workspace-gateway`'s own
  notes.md independently flagged 2 of these 5 (`projections.rs`'s pair) as "pre-existing... unrelated
  to this task" — this task's own investigation additionally found `heads.rs`'s 3, meaning the
  transitive-dependency clippy gate has been broken for any crate depending on `txtodo-store` for
  longer than previously documented. Left untouched per this task's hard constraint (stay inside
  `crates/txtodo-daemon/`, `crates/txtodo-cli/`, `tasks/`, root `todo.txt`) — fixing `txtodo-store`'s
  cognitive-complexity debt was not asked for by this backlog line and is a separate, larger
  refactor. `cargo clippy -p txtodo-daemon --lib --tests -- -D warnings 2>&1 | grep tokens.rs`
  returns nothing: this task's own two files produce zero clippy findings.

### What's left
Nothing in this backlog line's scope is left. Request-time enforcement of `workspace:` (and, for
that matter, of `read`/`write:*`/`project:`/`context:`/`file:`, none of which are enforced either)
remains the larger, pre-existing, separately-tracked MCP-auth-server milestone — unchanged by this
task, not newly deferred by it.
