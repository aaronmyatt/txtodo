#!/bin/sh
# zig cc as the linker for x86_64-unknown-linux-musl (static musl target).
# Cargo `linker` needs one bare executable, hence this wrapper:
# https://doc.rust-lang.org/cargo/reference/config.html#target
exec zig cc -target x86_64-linux-musl "$@"
