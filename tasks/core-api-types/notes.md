# Define the frozen public API types

Plan M1 "Public API (freeze this shape; details may grow)". Types only in this task; the functions land
with their own tasks. `crates/txtodo-core/**` is the shared kernel: frozen path, the fence asks on every write.

## Module layout (file budget 400 lines, so the crate is split from day one)
```
src/lib.rs        #![no_std] + extern crate alloc; #![forbid(unsafe_code)]; pub use of everything below
src/types.rs      Mode Span TokenKind Date Priority LineEnding Line LineKind Task OwnedLine File
src/quirks.rs     Quirks
src/error.rs      ParseError
src/ulid.rs       Ulid
src/urls.rs       (task core-url-detection)      src/scanner.rs, tokenize.rs (core-scanner-tokenize)
src/parse.rs      (core-parser-strict/lenient)   src/task.rs (core-task-views)
src/file.rs       (core-parse-file)              src/format.rs edit.rs diff.rs (later tasks)
```

## Shapes (from the plan, with the details it leaves open decided here)
```rust
pub enum Mode { Strict, Lenient }
pub struct Span { pub kind: TokenKind, pub start: usize, pub end: usize }   // byte offsets, UTF-8 safe
pub enum TokenKind { CompletionMarker, CompletionDate, CreationDate, Priority, Project, Context,
                     TagKey, TagValue, IdTag, Url, Text, Whitespace }       // == corpus/tokens.schema.json
pub struct Date { year: u16, month: u8, day: u8 }        // private fields; Date::new validates the calendar
pub struct Priority(u8);                                 // b'A'..=b'Z'; Priority::new(char) -> Option
pub enum LineEnding { Lf, CrLf, None }                   // None = last line without newline
pub struct Line<'a> { pub raw: &'a str, pub kind: LineKind<'a>, pub quirks: Quirks, pub ending: LineEnding }
pub enum LineKind<'a> { Blank, Task(Task<'a>) }
pub struct Task<'a> { pub completed: bool, pub completion_date: Option<Date>, pub creation_date: Option<Date>,
                      pub priority: Option<Priority>, pub description: &'a str }
pub struct OwnedLine { raw: String, quirks: Quirks, ending: LineEnding }   // + parsed view on demand
pub struct File { pub lines: Vec<OwnedLine>, pub bom: bool, pub ending: LineEnding, pub trailing_newline: bool }
pub struct Quirks(u16);  // consts: NO_COMPLETION_DATE PRIORITY_AFTER_X PRIORITY_AFTER_DATE TABS TRAILING_WS
                         //         INVALID_REF MIXED_ENDING  (+ room for more; never reuse a bit)
pub struct ParseError { pub rule: &'static str, pub byte: usize, pub message: &'static str }
pub struct Ulid([u8; 16]);
```

## Decisions (record in stack.md judgement calls if they hold)
- **No `ulid` crate.** Core needs parse + display only (~40 lines). Plan §0 asks before adding deps to core;
  a hand-rolled Crockford decoder avoids the ask. Spec: https://github.com/ulid/spec
- **No `bitflags`.** A `u16` newtype with named consts keeps core dependency-free.
- **`thiserror` only with `std`.** Core is `no_std`; `Display` is hand-written, `std::error::Error` impl is
  `#[cfg(feature = "std")]`. Ref: https://docs.rs/thiserror
- **Enums stay exhaustive** (no `#[non_exhaustive]`): exhaustive `match` over `TokenKind` is the point (constitution §3).
- **Dates are validated, not just shaped.** `2026-02-30` is a parse error in strict mode and a `Text` word
  in lenient mode. The ABNF fixes the shape; the comment there says the parser checks the calendar.

## Budgets that bite here
- Every function ≤ 60 lines, ≤ 5 params, nesting ≤ 3; every public item documented (`missing_docs` = deny).
- ≥ 2 assertions per fn: constructors `debug_assert!` their invariants (month ≤ 12, priority in A..=Z).
- No `unwrap`/`expect` outside tests (clippy denies). Use `?`, `ok_or`, `match`.
