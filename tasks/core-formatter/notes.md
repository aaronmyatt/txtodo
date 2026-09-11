# Byte-preserving formatter with field-level dirtiness

Design §2.2 rule 2: a line txtodo didn't change is written back byte-for-byte. Plan M1: "Formatter with
field-level dirtiness so untouched bytes are preserved." Rule 3: the formatter only ever emits the strict
grammar for the parts it rewrites.

## Model
```rust
pub struct OwnedLine { raw: String, ending: LineEnding, quirks: Quirks, dirty: Dirty }
struct Dirty(u8);   // COMPLETED | COMPLETION_DATE | CREATION_DATE | PRIORITY | DESCRIPTION
```
`OwnedLine::from_line(&Line)` starts clean. `Edit` (next task) is the only thing that sets bits. `format`:
1. `dirty == 0` → return `raw.clone()` (or write `raw` bytes straight through in `File::to_bytes`).
2. Otherwise re-parse `raw` (lenient) to find the byte range of the prefix, emit a fresh strict prefix from
   the (possibly edited) fields, then append the description bytes: unchanged bytes if `DESCRIPTION` is
   clean, the new description otherwise.

## Consequences to test
- Editing the priority of `x (A) 2026-09-11 t` (quirk PRIORITY_AFTER_X) normalises the prefix, because the
  prefix is rebuilt strictly; the quirk bit is cleared. Editing only the description leaves the quirky
  prefix alone. Document both in the fn doc comment.
- `TRAILING_WS` and `TABS` live in the description; a prefix-only edit keeps them.
- Line ending is never touched by `format`; `File::to_bytes` appends it.
- Blank lines: `format` of a Blank is `""`.

## Budget
`format` ≤ 60 lines by delegating `emit_prefix(fields) -> String` (≤ 30) and `prefix_len(raw) -> usize`.
