# Write specs/todotxt.abnf

Normative grammar. M1's hand-written parser and the test-only generated parser both derive from it;
M7's Lezer grammar is generated from it. `specs/**` is a frozen path: the fence asks on every write.

## Notation
RFC 5234 ABNF, with the RFC 7405 `%s` case-sensitive string form where needed.
Ref: https://www.rfc-editor.org/rfc/rfc5234 · https://www.rfc-editor.org/rfc/rfc7405

## Additions to the design §2.3 grammar
```abnf
; --- extension tags (Appendix A of the design; all optional) -------------------
ref-tag     = %s"ref:" slug
slug        = slug-start *63slug-char        ; max 64 chars total, no "/", no traversal
slug-start  = %x61-7A / DIGIT                ; a-z 0-9
slug-char   = slug-start / "." / "_" / "-"
; "." and ".." are syntactically slugs but are rejected by the parser as quirk invalid_ref.
; "/" is not in slug-char, so absolute paths and parent traversal cannot match.

id-tag      = %s"id:" ulid
ulid        = 26( DIGIT / %x41-48 / %x4A-4B / %x4D-4E / %x50-54 / %x56-5A )  ; Crockford base32, no I L O U
pri-tag     = %s"pri:" %x41-5A
date-tag    = ( %s"due:" / %s"t:" ) date
rec-tag     = %s"rec:" ["+"] 1*DIGIT ( "d" / "w" / "m" / "y" )
h-tag       = %s"h:1"
```
`word` stays as in §2.3; the tags above are *recognised forms* of `tag`, not new alternatives, so
unknown `key:value` still parses as a tag.

## Lenient mode (comments only; never emitted)
| Accepts | Quirk |
|---|---|
| `x` with no completion date | `no_completion_date` |
| `x (A) 2026-…` / `x 2026-… (A) …` | `priority_after_x` / `priority_after_date` |
| tabs, runs of spaces, trailing whitespace | `tabs` / `trailing_ws` |
| `+` or `@` at the very start of the line | (no quirk; it is a project/context by the spec) |
| bad ref slug | `invalid_ref` |

## Validation
```bash
# scratch only: cargo new abnf-check && cargo add abnf && parse the file with abnf::rulelist
```
Ref: https://docs.rs/abnf
