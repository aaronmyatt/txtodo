# tokenize sharing the parser scanner

Plan M1: "`tokenize` shares the parser's scanner; tokens must cover every byte (`Whitespace` included) so
highlighters can paint without gaps." The corpus oracles (`corpus/*.tokens.json`) define the expected output
exactly; read them before writing a line of code. Highlighters on every platform paint from these spans
(plan §1 #8), so token boundaries are a public contract.

## Scanner
```rust
pub(crate) enum Chunk { Ws { start: usize, end: usize }, Word { start: usize, end: usize } }
pub(crate) fn chunks(raw: &str) -> impl Iterator<Item = Chunk>   // by bytes; ' ' and '\t' are whitespace
```
Only ASCII space and tab split words (ABNF `SP`, lenient `tabs`). Unicode spaces are NONSP, part of words.

## Classification order per word (first match wins)
1. prefix rules, only while still in the prefix: `x` at byte 0 → CompletionMarker; a `date` right after it →
   CompletionDate; a `date` next → CreationDate (or CreationDate first if no `x`); `(A)` → Priority.
   Lenient positions (`x (A) date`, `x date (A)`) still tokenise as Priority; the *parser* records the quirk.
2. `is_url` → Url.
3. `id:` + 26 Crockford chars → IdTag (configurable key name comes with the daemon; hard-code `id` for now).
4. `+x` → Project; `@x` → Context (word must be longer than the sigil).
5. `key:value` with non-empty key and non-empty value → TagKey (`key:`) + TagValue.
6. otherwise Text. `note:` is Text (empty value). `X`, `(a)`, `2026-13-40` are Text.

Position matters: `X 2026-09-11 not done` has no `x`, so `2026-09-11` is not a date token: the prefix ended
at `X` (Text). Once a word is Text, everything after is description; dates there are Text.

## Invariant tests (assert in code, test in tests)
- `spans.first().start == 0`, each `span.end == next.start`, `spans.last().end == raw.len()`, no empty spans.
- `tokenize("")` → `[]`.
Until `parse_file` exists, a unit test reads `corpus/edge-cases.tokens.json` via `include_str!` (tests may;
core stays I/O-free). `just corpus` takes over in task core-corpus-test.
