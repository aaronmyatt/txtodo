# Task views: projects, contexts, tags, tag, id, ref_slug

Plan M1 API:
```rust
impl<'a> Task<'a> {
    pub fn projects(&self) -> impl Iterator<Item = &'a str>;
    pub fn contexts(&self) -> impl Iterator<Item = &'a str>;
    pub fn tags(&self) -> impl Iterator<Item = (&'a str, &'a str)>;
    pub fn tag(&self, key: &str) -> Option<&'a str>;   // first match
    pub fn id(&self) -> Option<Ulid>;
    pub fn ref_slug(&self) -> Option<&'a str>;         // validated per plan §3.2.1
}
```
Design §3 key decision: projects/contexts/tags are *views computed from the description on demand*, never
stored fields. That is what keeps the CRDT faithful (it syncs the description text, nothing derived).

## Rules
- Views walk `scanner::chunks(description)` and reuse the tokenizer's word classifier so a word is a project
  in exactly one place. `learn C++ +cpp` → one project `cpp`; `mail bob@example.com` → no context.
- Returned `&str` excludes the sigil: `projects()` yields `cpp`, not `+cpp`. `tags()` yields `("due", "2026-09-15")`.
- `tags()` includes `id` and `ref` and `pri`: they are ordinary tags to this API; `id()`/`ref_slug()` are typed
  conveniences. `tags()` excludes URLs (a URL is not a tag even though it contains a colon).
- Slug validation is one function `pub fn is_valid_slug(s: &str) -> bool` in task.rs, ≤ 20 lines, and it is
  what core-fuzz-targets fuzzes for path traversal (plan §5 security checklist).
- The parser sets `INVALID_REF` when a `ref:` tag is present and invalid, so `Line.quirks` is complete
  without re-walking. `ref_slug()` re-validates anyway (cheap, and views must not trust quirks).
