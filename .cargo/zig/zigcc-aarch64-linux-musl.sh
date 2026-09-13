#!/bin/sh
# zig cc as the linker for aarch64-unknown-linux-musl (static musl target, arm64).
# Cargo `linker` needs one bare executable, hence this wrapper:
# https://doc.rust-lang.org/cargo/reference/config.html#target
exec zig cc -target aarch64-linux-musl "$@"
