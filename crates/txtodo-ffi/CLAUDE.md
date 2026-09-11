# txtodo-ffi

## Purpose
uniffi, wasm-bindgen, cbindgen bindings over core and query. Plan M9.

## Public interface
`tokenize`, `parse_line`, `DaemonHandle`.

## Invariants
- The only crate where `unsafe` is permitted.
- May depend only on: txtodo-core, txtodo-query.
