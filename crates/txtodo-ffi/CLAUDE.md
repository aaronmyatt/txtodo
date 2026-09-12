# txtodo-ffi

## Purpose
uniffi, wasm-bindgen, cbindgen bindings over core and query. Plan M9.

## Public interface
`tokenize`, `parse_line`, `DaemonHandle`.

Grown (2026-09-12), `wasm32-unknown-unknown` only (`crates/txtodo-ffi/src/wasm.rs`, gated
`#[cfg(target_arch = "wasm32")]`): `parse_line_strict(raw: &str) -> JsValue` (`{ ok: true }` or
`{ ok: false, rule, byte, message }`, wraps `txtodo_core::parse_line(_, Mode::Strict)` for the
desktop edit popover's inline error, `tasks/desktop-edit-popover/notes.md`) and
`diff_text(a: &str, b: &str) -> JsValue` (array of `{ op: "equal"|"insert"|"delete", text }`
segments, full coverage of both `a`/`b` — wraps `txtodo_core::diff_text` for the conflict-review
`DiffView`, `tasks/desktop-conflict-review/notes.md`). Both are thin `JsValue` wrappers; the tested
logic lives in the target-independent `parse_check` (`StrictCheck`/`check_strict`) and `diff_view`
(`DiffOp`/`DiffSegment`/`diff_segments`) modules, run by plain `cargo test -p txtodo-ffi`.

## Invariants
- The only crate where `unsafe` is permitted.
- May depend only on: txtodo-core, txtodo-query; plus, `wasm32-unknown-unknown`-only,
  `wasm-bindgen` and `js-sys` (2026-09-12, for the wasm exports above — needs human sign-off before
  merging to main per this repo's no-new-dependency convention).
