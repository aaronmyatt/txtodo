# Hand-written recursive-descent parser, strict mode

Grammar: `specs/todotxt.abnf` (normative). Plan M1 wants a hand-written parser and, separately, a second
parser generated from the ABNF for differential tests (task core-pest-differential). Both must agree on the
corpus and on 10 000 generated lines.

## Signature (plan, frozen)
```rust
pub fn parse_line(raw: &str, mode: Mode) -> Result<Line<'_>, ParseError>;
```
Strict mode: any deviation from the ABNF is `Err`. Lenient (next task) turns the listed deviations into quirks
and everything else into "the whole line is a description".

## Structure (each fn ≤ 60 lines; nesting ≤ 3)
```
parse_line       → blank? | parse_completed | parse_incomplete, then description = raw[cursor..]
parse_completed  → expects b'x', SP, date, SP, [date SP]
parse_incomplete → [priority SP] [date SP]
take_date        → 10 bytes YYYY-MM-DD, then Date::new (calendar check) → ParseError { rule: "date" }
take_priority    → "(" A-Z ")" → ParseError { rule: "priority" }
```
Description is `&raw[cursor..]` verbatim: byte-preserving round trip (design rule 2) depends on never copying
or trimming it. Strict grammar has no trailing SP (`description = word *(SP word)`), so trailing whitespace
and tabs are strict errors and lenient quirks.

## Error contract
`ParseError { rule, byte, message }`: `rule` is the ABNF rule name (`"priority"`, `"date"`, `"completed"`),
`byte` the offset where matching failed. Design §6.3: structured errors point at the spec rule so an agent
that sends `(a) task` learns why. Test that `byte` is exact for every failing case.

## Not errors (design §2.4)
`X 2026-09-11 not done` is an incomplete task whose description starts with `X`. `(a) task` is plain text.
Errors are things like `x2026-09-11 t` (no SP), `(A)task`, `2026-02-30 t`, and `x 2026-09-11 (A) t` in strict.
