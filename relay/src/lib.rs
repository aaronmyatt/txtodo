//! Reference relay binary (M8, design §4.5): stores encrypted op blobs by `(group, device)`,
//! forwards push wake-ups. Deliberately has zero `txtodo-*` dependencies — see
//! tests/no_txtodo_deps.rs — the relay is untrusted and must never be able to parse ciphertext
//! it stores (design §4.6). Split into a library so `tests/*.rs` and `main.rs` share one crate
//! root, same as `txtodo-daemon`.
#![forbid(unsafe_code)]

pub mod bounds;
pub mod clock;
pub mod config;
pub mod http;
pub mod push;
pub mod ratelimit;
pub mod retention;
pub mod store;
