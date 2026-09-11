# Edit builder and apply

Plan M1: "Mutation API: every mutation returns a new OwnedLine; the formatter only rewrites the fields it
touched." The CLI (M2) and the daemon's `Apply` (M3) are built on this; the MCP `todo_edit` patch (design
§6.3: `priority`, `due`, `append`, `replace`) maps 1:1 onto these ops.

## API
```rust
#[derive(Default)]
pub struct Edit { ops: Vec<Op> }           // private; builder methods push ops, applied in order
impl Edit {
    pub fn set_priority(self, p: Priority) -> Self;   pub fn clear_priority(self) -> Self;
    pub fn set_description(self, s: &str) -> Self;
    pub fn set_tag(self, key: &str, value: &str) -> Self;   pub fn remove_tag(self, key: &str) -> Self;
    pub fn append(self, text: &str) -> Self;   pub fn prepend(self, text: &str) -> Self;
    // complete(today) / uncomplete() come in task core-complete-pri
}
pub fn apply(line: &OwnedLine, edit: &Edit) -> OwnedLine;
```

## Semantics (decide once, test each)
- Ops apply in builder order; later ops see earlier results.
- `set_tag` on an existing key rewrites the value in place (position preserved; other tools and humans
  rely on order). Missing key → append ` key:value`. Value must be NONSP: `debug_assert!` + `Result`? Keep the
  builder infallible and validate in `apply`: an invalid value is a no-op with the op dropped? No — silent
  drops lose intent. `apply` returns `OwnedLine`; make `set_tag` itself return `Result<Self, EditError>` so the
  caller learns at build time. Same for `set_description` containing `\n`.
- `remove_tag` removes the first match and one adjacent SP (prefer the preceding one), so `a due:x b` → `a b`.
- `append`/`prepend` add exactly one SP; on an empty description no SP.
- `apply` compares field-by-field with the parsed original and sets only the changed Dirty bits, so an
  `Edit` that sets the same priority again leaves the line byte-identical (design: no-op saves make no ops).

## Tests
Plan M1 property "apply then apply inverse is identity for priority/complete" lands in core-proptest; here,
unit tests per op plus the no-op identity.
