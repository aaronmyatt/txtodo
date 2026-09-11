# Add fuzz/ with a stub parse_line target so just fuzz runs

Plan M0 acceptance: "`just fuzz parse_line 60` runs (target may be a stub that does nothing yet)."
Plan conventions: "fuzz targets under `fuzz/`" — cargo-fuzz's default is per crate, so the path is
`crates/txtodo-core/fuzz/`. M1 adds `parse_file` and the real bodies.

## Layout after `cargo fuzz init` (run from crates/txtodo-core)
```
crates/txtodo-core/fuzz/
  Cargo.toml            # depends on txtodo-core by path; NOT a workspace member
  fuzz_targets/parse_line.rs
```
Ref: https://rust-fuzz.github.io/book/cargo-fuzz/tutorial.html

## Stub body
```rust
#![no_main]
use libfuzzer_sys::fuzz_target;
// Stub until M1: prove the harness runs. M1 replaces the body with
// `let _ = txtodo_core::parse_line(s, Mode::Lenient);` for both modes.
fuzz_target!(|data: &[u8]| {
    let _ = core::str::from_utf8(data);
});
```

## Workspace exclusion
The root `Cargo.toml` uses `members = ["crates/*"]`; the fuzz crate lives one level deeper, so cargo
would otherwise error "package believes it's in a workspace when it's not". Add
`exclude = ["crates/txtodo-core/fuzz"]`. Root `Cargo.toml` is frozen: the fence asks.
Ref: https://doc.rust-lang.org/cargo/reference/workspaces.html#the-exclude-field

## Lints
The fuzz crate is outside the workspace, so the workspace `[lints]` table does not apply. Fine: it is
test scaffolding, but keep `#![forbid(unsafe_code)]` out (libfuzzer-sys needs none, but the macro
expands to an `extern "C"`).
