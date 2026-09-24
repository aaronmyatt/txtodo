//! uniffi, wasm-bindgen, cbindgen bindings over core and query.
// unsafe is permitted here only: FFI boundary (plan §0 Conventions).

pub mod diff_view;
pub mod parse_check;
pub mod shared;
#[cfg(target_arch = "wasm32")]
mod wasm;
