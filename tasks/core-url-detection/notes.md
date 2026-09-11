# URL detection before tag detection

Design §2.4: `see https://example.com/x` is not a tag; "URL schemes are detected before tag parsing; the
scheme list is configurable". Plan M1: schemes `http https mailto ftp ssh file tel sms`, configurable via a
`&[&str]` parameter, default list in `core::urls::DEFAULT_SCHEMES`.

## Rule
A word is a `Url` iff `word[..i]` (ASCII case-insensitive) is in `schemes` and `word[i] == ':'` and there is at
least one more byte. Otherwise it goes on to tag/project/context/plain classification. So:
- `https://user@example.com/x` → Url (corpus tags.txt line 9), even though it contains `@`.
- `note:` → not a URL (no bytes after `:` and `note` is not a scheme) → plain `Text` (a tag needs a value).
- `color:blue` → `TagKey`+`TagValue` (color is not a scheme).
- `file:notes.md` → Url. That is the documented cost of `file` in the default list; users can drop it.

## API
```rust
/// Schemes recognised by default; order is irrelevant. Ref: https://www.iana.org/assignments/uri-schemes
pub const DEFAULT_SCHEMES: &[&str] = &["http", "https", "mailto", "ftp", "ssh", "file", "tel", "sms"];
/// True when `word` starts with `<scheme>:` for a scheme in `schemes` and has at least one byte after the colon.
pub fn is_url(word: &str, schemes: &[&str]) -> bool
```
Case-insensitivity: `str::eq_ignore_ascii_case` on the slice before `:`; no allocation.

## Where it plugs in
The scanner (next task) classifies each word: `is_url` first, then `+`/`@`/`key:value`/plain. `parse_line` and
`tokenize` take `schemes: &[&str]`; `tokenize(raw)` / `parse_line(raw, mode)` keep the plan's signatures by
delegating to `*_with_schemes(…, DEFAULT_SCHEMES)`.
