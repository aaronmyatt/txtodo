# txtodo-core

## Purpose
Parser, tokenizer, model, byte-preserving formatter, diff. Plan M1.

## Public interface
Plan M1 shape (frozen): `parse_line`, `tokenize`, `parse_file`, `File::to_bytes`, `Edit`/`apply`, `diff_lines`, `diff_text`.
Grown details (2026-09-11): `parse_line_with_schemes`, `tokenize_with_schemes`, `urls::{DEFAULT_SCHEMES, is_url}`,
`Quirks` (u16 bitset, `ALL` names), `ParseError { rule, byte, message }`, `Ulid`, `Date::{new, parse}`, `Priority`,
`OwnedLine::{from_bytes, raw, parse, ending, quirks}`, `Prefix` + `emit_prefix` + `description_start` (formatter),
`lint_findings` (what `txtodo lint` reports, moved here so the daemon's `Lint` RPC runs the same
check), `line_length::{LINE_LENGTH_HINT, over_length_hint, visible_chars}` (2026-09-20: the one advisory
line-length measure — visible `char`s, own `id:` tag not counted — that `txtodo lint`, the TUI and
the desktop editor share), `Edit::{set_priority, clear_priority, set_description, set_tag, remove_tag, append, prepend, complete, uncomplete}`
(`Result<_, EditError>` where input is validated), `LineDiff`, `TextEdit`, `is_valid_slug`, `SLUG_MAX_LEN`.
`query::{matches, GOLDEN}` (2026-09-25, task `tui-revamp/shared-core`): the one line-search matcher
(AND terms, case-insensitive substring, `-term` excludes, `is:open`/`is:done`) that `txtodo list`, MCP,
the TUI and (through `txtodo-ffi`) desktop share; `GOLDEN` is the table every wrapper is tested against.
Mode contract: strict = the ABNF exactly (a bare `x` is description text); lenient = total over `&str`, quirks recorded.

## Invariants
- No I/O, no clocks, no async dependency. `no_std` + `alloc` must build.
- `format(parse(x)) == x` for every corpus line; `tokenize` covers `[0, len)` exactly.
- Never emits a construct the todo.txt spec does not define.
- A second parser generated from `specs/todotxt.abnf` (tests/differential.rs) must agree with `parse_line(_, Strict)`.
- Tests: `cargo test -p txtodo-core` (unit, corpus, edge cases, proptest, differential); `just fuzz <target> <secs>`; `just bench-check`; `just no-std`.
- May depend only on: nothing in the workspace.
