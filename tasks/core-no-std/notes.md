# no_std + alloc feature flag builds

Plan M1: "`no_std` + `alloc` feature flag builds (no I/O in this crate anyway)." Design §3: a `no_std`-
compatible crate, no I/O, no clocks. This is what makes the WASM and embedded bindings (M7/M9/M10) cheap.
Ref: https://doc.rust-lang.org/reference/names/preludes.html#the-no_std-attribute

## Shape
```toml
[features]
default = ["std"]
std = []
```
```rust
#![cfg_attr(not(feature = "std"), no_std)]
extern crate alloc;
use alloc::{string::String, vec::Vec};
```
Everything allocating uses `alloc::`; `core::str`, `core::fmt`. Only `impl std::error::Error for ParseError`
is std-gated. `parse_file(bytes: &[u8]) -> File` needs no std.

## Proof
A build for a target with no std at all: `thumbv7em-none-eabihf` (no OS). `rustup target add` once, locally
and in CI (`dtolnay/rust-toolchain` with `targets:`). A plain `--no-default-features` on the host is not
enough: std is still linkable there and a stray `std::` path would go unnoticed.

## Order
Do this *last* in M1 rather than first: writing `alloc::` everywhere from day one is fine too, but the
`thumbv7em` build is the real check and it only means something once the code exists. Task core-api-types
already puts `#![no_std]` in lib.rs so the habit starts early.
