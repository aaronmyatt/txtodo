# parse_file: BOM, LF/CRLF per file, mixed endings, missing trailing newline

Design §2.2 rule 6 (blank lines are entries) and rule 7 (hygiene preserved, never imposed). Plan M1:
`parse_file(bytes: &[u8]) -> File` detects BOM, line endings, trailing newline; `File { lines, bom, ending,
trailing_newline }`; `File::to_bytes()`.

## Algorithm
1. `bom = bytes.starts_with(&[0xEF, 0xBB, 0xBF])`; strip it for parsing, re-emit it in `to_bytes`.
2. Split on `\n`. For each piece, if it ends with `\r`, the line's ending is `CrLf` and the `\r` is not part
   of `raw`. The last piece: if the input ended with `\n`, `trailing_newline = true` and the final empty piece
   is *not* a line; otherwise the last piece is a line with `LineEnding::None`.
3. `ending` (file-level) = the majority ending among lines (ties → `Lf`). A line whose ending differs gets
   `Quirks::MIXED_ENDING`. `to_bytes` always writes each line's *own* ending, so the file-level value is
   advice for new lines only (the daemon uses it when appending).
4. Blank pieces become `LineKind::Blank` entries; they count for line numbers (rule 6).

## Invalid UTF-8 (decide now, record in stack.md)
Option A: `OwnedLine::raw: String` and `parse_file` returns `Result<File, Utf8Error>` — simple, but one bad
byte makes the whole file unreadable, violating "never lose text".
Option B (recommended): `OwnedLine` stores `Vec<u8>`; parsing a line first tries `from_utf8`; on failure the
line is `LineKind::Blank`-like `Opaque` that round-trips verbatim and is reported by `txtodo lint`.
Needs a `LineKind::Opaque` variant — a small API addition, allowed ("details may grow").

## Round-trip contract (the test)
For each hygiene file in `corpus/`: `parse_file(bytes).to_bytes() == bytes`. Use `include_bytes!` in tests.
This is the first half of `just corpus` (task core-corpus-test); the second half is tokens.
