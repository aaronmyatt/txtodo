# proptest strategies and the four properties

Plan M1: "`proptest` strategies for valid tasks; properties: `format(parse(x)) == x` for corpus lines;
`parse(format(t)) == t` for generated tasks; `tokenize` covers `[0, len)` exactly; `apply` then `apply`
inverse is identity for priority/complete." Constitution §7: anything with an invariant (round-trip,
ordering, conservation) gets a property-based test. Ref: https://docs.rs/proptest

## Strategies (`crates/txtodo-core/tests/strategies.rs`, shared inside this crate's tests only)
```rust
fn date() -> impl Strategy<Value = Date>          // 1970..=2100, valid day for month (leap years included)
fn priority() -> impl Strategy<Value = Priority>  // 'A'..='Z'
fn word() -> impl Strategy<Value = String>        // 1..12 NONSP chars, Unicode classes mixed, no leading + @ and no ':'
fn project()/context()/tag()                       // "+" word, "@" word, word ":" word
fn description() -> impl Strategy<Value = String> // 1..8 words joined by one SP, at least one plain word
fn strict_line() -> impl Strategy<Value = String> // [ "x" SP date SP [date SP] | [priority SP] [date SP] ] description
fn any_line() -> impl Strategy<Value = String>    // strict_line ∪ lenient mutations (add tabs, trailing ws, move priority)
```

## Properties
1. `format(parse_lenient(x)) == x` for `any_line()` and every corpus line (byte identity, untouched line).
2. `parse(format_strict(t)) == t` where `t` is a generated `Task` and equality is field-wise (description,
   dates, priority, completed).
3. `tokenize(s)` for `s: any::<String>()` (truly arbitrary): contiguous, starts at 0, ends at `s.len()`, no
   empty spans, and every `start`/`end` is a char boundary (`s.is_char_boundary`).
4. Inverse edits: for `strict_line()` without priority, `apply(clear)(apply(set_priority(p)))` is
   byte-identical; for open lines, `uncomplete(complete(d))` is byte-identical.

## Config
`ProptestConfig { cases: 1000, .. }` locally; CI uses the default 256 via `PROPTEST_CASES`. Shrinking output
goes to `proptest-regressions/` in the crate: commit it (it is a test fixture, small).
