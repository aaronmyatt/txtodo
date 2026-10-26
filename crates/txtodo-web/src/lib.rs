//! The browser peer's host (ADR 0042, task `web-pwa`): what `txtodod` is to the desktop app, run
//! inside a dedicated Web Worker. It holds each list as a `txtodo_engine` document over
//! `txtodo-store`'s op log (SQLite in OPFS there), and answers the desktop frontend's file
//! commands as JSON (`host::Host::call`). Plain Rust over the engine and the store, so the native
//! build runs its tests; `wasm` binds it for the Worker. Sync, keys and pairing are not here yet.

pub mod clock;
pub mod doc;
pub mod dto;
pub mod error;
pub mod host;
mod mutation_dto;

// wasm-bindgen's generated glue is `unsafe` inside; nothing hand-written here is.
// Ref: https://wasm-bindgen.github.io/wasm-bindgen/reference/attributes/index.html
#[cfg(all(target_family = "wasm", target_os = "unknown"))]
mod wasm;

#[cfg(test)]
mod host_tests;
