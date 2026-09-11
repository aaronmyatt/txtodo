# Op model in txtodo-model: Hlc, Op, OpKind, Principal (plan M3)

Plan M3 gives the shapes verbatim; this crate is the one the CRDT (M4) reuses, so nothing here is
"for now". Consumers: `txtodo-store` (rows), `txtodo-daemon` (reconciler emits, actor applies),
M4 `txtodo-crdt` (maps OpKind to Loro ops), `txtodo log`/`blame` (Principal display).

## Shape
```rust
pub struct Hlc { pub wall_ms: u64, pub counter: u16, pub device: DeviceId }
pub struct Op { pub id: OpId, pub hlc: Hlc, pub principal: Principal, pub file: FilePath, pub kind: OpKind }
pub enum Field { Completed, CompletionDate, CreationDate, Priority, Deleted, Quirks }
pub enum FieldValue { Bool(bool), Date(Option<Date>), Priority(Option<Priority>), Quirks(Quirks) }
```
`TextEdit` is `txtodo_core::TextEdit` (char-level, M1) — do not redeclare it.
Invalid states unrepresentable: `Field`/`FieldValue` pairing is checked by a constructor
`SetField::new(field, value) -> Result<_, OpError>` and asserted in `debug_assert!` on apply.

## HLC
Ref: Kulkarni et al., "Logical Physical Clocks" https://cse.buffalo.edu/tech-reports/2014-04.pdf
`tick(now_ms)`: `wall = max(now_ms, self.wall_ms)`; same wall → `counter + 1` (error at `u16::MAX`),
else counter 0. The clock is injected (`Clock` trait, stack.md fakes); the 5-minute skew guard is M4.

## Codec
`serde` + `postcard` (https://docs.rs/postcard). One codec for store payload and M4 wire; the wire
adds a version field, the store row has `payload BLOB` only. Adding both crates needs the human's
yes (plan §0). Enums are `#[non_exhaustive]`-free: exhaustive `match` is the enforcement (stack.md).

## Budgets
Every constructor asserts its inputs; `FilePath::new` rejects `..`, absolute paths, backslashes
(validate — it is external input when it arrives over gRPC).
